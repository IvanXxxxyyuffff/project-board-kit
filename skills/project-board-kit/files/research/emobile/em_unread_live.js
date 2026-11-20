// t8 · 未读消息 1 分钟级近实时（短连接，不顶号）
// 形态：绑定 → 只发一次 getConversation → 落 unread_snapshot.json → 重算
//       widget_data.json.signals → 立刻 close。全程硬上限 8s，绝不常驻 WS。
// 绝不调用 /emp/passport/login、/emp/api/msg/gettoken，令牌失效只记日志退出。
// 令牌字段从 api/msgconn.json 动态读取（即占位名 msg_token 指向的字段），
// ws_url / authcid / msg_domain / msg_user_id / msg_udid 同样取自 msgconn.json。
// 用法：
//   node em_unread_live.js --once          短连接一轮，输出 UNREAD_LIVE ok items=N ms=...
//   node em_unread_live.js --render-only   只从现有快照重算 signals（不连 WS，验收/调试用）
'use strict';
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const DIR = path.resolve(__dirname);                  // <ROOT>/research/raw/emobile
const RS = path.resolve(DIR, '..', '..');             // <ROOT>/research
const API = path.join(DIR, 'api');
const SNAP = path.join(DIR, 'unread_snapshot.json');
const WDATA = path.join(RS, 'widget_data.json');
const LOG = path.join(RS, 'unread_live.log');

const ONCE = process.argv.includes('--once');
const RENDER_ONLY = process.argv.includes('--render-only');
const T0 = Date.now();
const HARD_MS = 8000;                                 // <10s 硬上限
const RESOURCE = 'ul' + crypto.randomBytes(3).toString('hex'); // 独立 resource ul<hex>

const readJson = (p, d) => { try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch (e) { return d; } };
function writeAtomic(p, text) {
  const tmp = p + '.ul.tmp';
  fs.writeFileSync(tmp, text, 'utf8');
  fs.renameSync(tmp, p);
}
function appendLog(s) {
  try { fs.appendFileSync(LOG, new Date().toISOString() + ' ' + s + '\n', 'utf8'); } catch (e) {}
}
const elapsed = () => Date.now() - T0;

// ---------- 连接参数 ----------
const conn = readJson(path.join(API, 'msgconn.json'), {}) || {};
const gt = readJson(path.join(API, 'gettoken.json'), {}) || {};
const META = new Set([
  'at', 'loginid', 'msg_user_id', 'msg_udid', 'msg_domain',
  'ws_host', 'ws_port', 'ws_url', 'authcid', 'errmsg', 'api_ip'
]);
// 不硬编码令牌字段名：先按语义 key 找，再退化为「唯一的非元数据长字符串」。
function pickToken(o) {
  if (!o || typeof o !== 'object') return '';
  for (const [k, v] of Object.entries(o)) {
    if (!/token|password|passwd|pwd|secret|ticket/i.test(k)) continue;
    if (typeof v === 'string' && v.length >= 16) return v;
  }
  for (const [k, v] of Object.entries(o)) {
    if (META.has(k)) continue;
    if (typeof v === 'string' && v.length >= 16 && !/^[\d.\-:/]+$/.test(v)) return v;
  }
  return '';
}
// 主路径与 em_full_pull.js / em_realtime.js 一致：直接读 msgconn 的令牌字段占位；
// pickToken 只作为「换了字段名/旧文件」兜底，避免轻量通道因为字段改名直接失效。
const TOKEN = conn.msg_token || gt.msg_token || pickToken(conn) || pickToken(gt) || '';
const DOMAIN = conn.msg_domain || 'weaver';
const SERVICE = conn.ws_url || ('ws://' + (conn.ws_host || '<LAN_HOST>') + ':' + (conn.ws_port || 7070) + '/ws/');
const UID = String(conn.msg_user_id || '<UID>');
const AUTHCID = conn.authcid || (UID + '|' + (conn.msg_udid || 'c'));

// ---------- 与 t3 build_widget.mjs 完全一致的 signals 口径 ----------
function fixMojibake(str) {
  const s = String(str || '');
  if (!/[\u0080-\u00FF]{2,}/.test(s)) return s;
  try {
    const back = Buffer.from(s, 'latin1').toString('utf8');
    const bad = (back.match(/\uFFFD/g) || []).length;
    return bad > back.length / 8 ? s : back;
  } catch (e) { return s; }
}
function unreadSignals(items) {
  const me = UID, now = Date.now();
  const byConv = new Map();
  for (let i = 0; i < items.length; i++) {
    const c = items[i];
    if (String(c.isdel) === '1') continue;
    let key = c.convKey || (String(c.targettype) + ':' + String(c.targetid));
    if (!c.convKey && c.targetid == null) key += '|' + (c.msgid != null ? String(c.msgid) : String(i));
    const prev = byConv.get(key);
    if (!prev || Number(c.lasttime || 0) >= Number(prev.lasttime || 0)) byConv.set(key, c);
  }
  const rows = [];
  for (const c of byConv.values()) {
    const ts = Number(c.lasttime || 0);
    if (!ts || now - ts > 48 * 3600 * 1000) continue;
    if (me && String(c.sendId || '') === me) continue;
    const n = Number(c.unreadcount || 0);
    let body = fixMojibake(String(c.msgcontent || c.content || '')).replace(/\s+/g, ' ').trim();
    body = body.replace(/^[^:：]{1,12}[:：]\s*/, '');
    if (/^\[(图片|文件|语音|视频|表情)\]$/.test(body)) body = '发来' + body.slice(1, -1);
    const plain = body.replace(/[\s\p{Extended_Pictographic}~!@#$%^&*()_+\-=\[\]{};':"\\|,.<>\/?·。，、！？…“”‘’（）《》]/gu, '');
    if (plain.length < 2) continue;
    const name = fixMojibake(String(c.targetname || c.groupName || '')).trim() || '未命名会话';
    const isPrivate = String(c.targettype) === '0';
    const dt = new Date(ts);
    const models = (body + ' ' + name).match(/(?<![A-Za-z0-9])(?:[A-Za-z]{2})?\d{4,6}[A-Za-z]?(?![A-Za-z0-9])/g) || [];
    rows.push({
      t: String(dt.getHours()).padStart(2, '0') + ':' + String(dt.getMinutes()).padStart(2, '0'),
      sender: name, src: 'EM', kind: isPrivate ? 'direct' : 'group', conv: isPrivate ? '' : String(c.targetname || name),
      text: (n > 0 ? n + '条未读 · ' : '') + (body.length > 52 ? body.slice(0, 52) + '…' : body),
      model: models[0] || '',
      ts, unread: n > 0 ? 1 : 0
    });
  }
  rows.sort((a, b) => (b.unread - a.unread) || (b.ts - a.ts));
  return rows.slice(0, 3).map(s => ({ t: s.t, sender: s.sender, text: s.text, model: s.model, src: s.src || 'EM', kind: s.kind || 'group', conv: s.conv || '' }));
}

// 重算 widget_data.json 的 signals（不改 build_widget.mjs；原子替换，mtime 变化触发挂件热载）
// 企微本地库（与 E-Mobile 同一分钟的短刷新）：spawn python 重建 wxwork_chat.json，
// 再把「近 3 小时别人发的最近一条」合并进 signals，两源并存。
const WXU = path.join(WDATA ? path.dirname(WDATA) : '.', 'wxwork_unread.json');
function refreshWx() {
  try {
    const py = process.env.FRD_PYTHON || 'python';
    const script = path.join(path.dirname(WXU), 'wxwork_unread_build.py');
    if (!fs.existsSync(script)) return;
    const r = require('child_process').spawnSync(py, ['-X', 'utf8', script, '--hours', '24', '--json', WXU],
      { timeout: 25000, windowsHide: true, encoding: 'utf8' });
    appendLog('wx refresh rc=' + (r.status === null ? 'timeout' : r.status) + ' ' + String(r.stdout || '').trim().split('\n').pop().slice(0, 90));
  } catch (e) { appendLog('wx refresh err=' + e.message); }
}
function wxRows() {
  try {
    const d = readJson(WXU, null);
    return (d && d.rows) || [];
  } catch (e) { return []; }
}
function mergeSignals(wx, em) {
  const seen = new Set(), out = [];
  const push = (r) => {
    const k = String(r.text || '').slice(0, 24);
    if (!k || seen.has(k)) return;
    seen.add(k);
    out.push(r);
  };
  wx.forEach(push); em.forEach(push);
  return out.slice(0, 3);
}
function refreshWidget(items) {
  const em = unreadSignals(items);
  const signals = mergeSignals(wxRows(), em);
  const d = readJson(WDATA, null);
  if (!d || typeof d !== 'object') {
    appendLog('render skip reason=widget_data-missing');
    return signals;
  }
  d.signals = signals;
  d.signalsMode = signals.length ? 'unread' : 'empty';
  writeAtomic(WDATA, JSON.stringify(d));
  appendLog('render signals=' + signals.length + ' mode=' + d.signalsMode + ' ms=' + elapsed());
  return signals;
}

// 快照字段与 t5 em_full_pull.js 一致；convKey 保证 t3 去重口径可直接用
function writeSnapshot(cv) {
  const un = cv.filter(c => Number(c.unreadcount || 0) > 0).map(c => ({
    targetname: c.targetname, targettype: c.targettype, lasttime: c.lasttime,
    msgcontent: c.msgcontent || c.content, sendId: c.sendId, unreadcount: Number(c.unreadcount || 0),
    targetid: c.targetid, msgid: c.msgid || c.lastmsgid || '',
    convKey: String(c.targettype) + ':' + String(c.targetid), isdel: c.isdel || ''
  }));
  writeAtomic(SNAP, JSON.stringify({ at: new Date().toISOString(), total: un.length, items: un }));
  appendLog('snapshot total=' + un.length + ' ms=' + elapsed());
  return un;
}

// ---------- 纯渲染轮：不连 WS，用于验收注入 / 断网降级 ----------
if (RENDER_ONLY) {
  const snap = readJson(SNAP, { items: [] }) || { items: [] };
  const items = Array.isArray(snap.items) ? snap.items : [];
  const signals = refreshWidget(items);
  console.log('UNREAD_LIVE render-only items=' + items.length + ' signals=' + signals.length + ' ms=' + elapsed());
  process.exit(0);
}

// ---------- --once 短连接 ----------
if (!ONCE) {
  console.log('UNREAD_LIVE fail reason=missing---once ms=' + elapsed());
  process.exit(2);
}
if (!TOKEN || !SERVICE || !AUTHCID) {
  appendLog('fail reason=token-missing (no login/gettoken attempted)');
  console.log('UNREAD_LIVE fail reason=token-missing ms=' + elapsed());
  process.exit(2);
}

const b64 = s => Buffer.from(s, 'utf8').toString('base64');
const ub64 = s => Buffer.from(s, 'base64').toString('utf8');
const sha1 = b => crypto.createHash('sha1').update(b).digest();
const hmac = (k, d) => crypto.createHmac('sha1', k).update(d).digest();
const pbkdf2 = (p, s, i) => crypto.pbkdf2Sync(Buffer.from(p, 'utf8'), s, i, 20, 'sha1');
const xor = (a, b) => { const o = Buffer.alloc(a.length); for (let k = 0; k < a.length; k++) o[k] = a[k] ^ b[k]; return o; };
const field = (s, n) => { const m = new RegExp('(?:^|,)' + n + '=([^,]*)').exec(s); return m ? m[1] : null; };
function decode(s) {
  return String(s).replace(/&quot;/g, '"').replace(/&apos;|&#39;|&#x27;/gi, "'")
    .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
}
function parseIQBody(d) {
  const qm = /<query[^>]*>([\s\S]*?)<\/query>/.exec(d);
  if (!qm) return null;
  try { return JSON.parse(decode(qm[1]).trim()); } catch (e) { return null; }
}

let ws = null, hard = null, done = false;
const st = { phase: 0, cnonce: '', cbare: '', needSession: false };
const send = x => { try { ws.send(x); } catch (e) {} };

function finish(reason, items, code) {
  if (done) return;
  done = true;
  if (hard) clearTimeout(hard);
  const ms = elapsed();
  const n = items == null ? 0 : items.length;
  if (reason === 'OK') {
    appendLog('ok resource=' + RESOURCE + ' items=' + n + ' ms=' + ms);
    console.log('UNREAD_LIVE ok items=' + n + ' ms=' + ms);
  } else {
    // 令牌失效 / 断链：只写日志，绝不触发 login 或 gettoken
    appendLog('fail reason=' + reason + ' resource=' + RESOURCE + ' ms=' + ms);
    console.log('UNREAD_LIVE fail reason=' + reason + ' ms=' + ms);
  }
  try { ws.close(); } catch (e) {}
  setTimeout(() => process.exit(code), 40);
}

function queueConv() {
  st.phase = 5;
  send("<iq id='g_conv' xmlns='http://weaver.com.cn/getConversation' type='get' to='" + DOMAIN + "'><query>{}</query></iq>");
}

function handle(d) {
  if (st.phase === 0 && d.indexOf('stream:features') >= 0) {
    st.phase = 1; st.cnonce = crypto.randomBytes(12).toString('hex');
    st.cbare = 'n=' + AUTHCID + ',r=' + st.cnonce;
    send("<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='SCRAM-SHA-1'>" + b64('n,,' + st.cbare) + '</auth>');
    return;
  }
  if (st.phase === 1 && /<challenge/.test(d)) {
    const mm = /<challenge[^>]*>([^<]*)<\/challenge>/.exec(d);
    const sf = ub64(mm ? mm[1] : '');
    const r = field(sf, 'r'), sB = field(sf, 's'), iter = parseInt(field(sf, 'i') || '4096', 10);
    if (!r || !sB) return finish('CHALLENGE_BAD', null, 1);
    const woProof = 'c=biws,r=' + r;
    const authMsg = st.cbare + ',' + sf + ',' + woProof;
    const salted = pbkdf2(TOKEN, Buffer.from(sB, 'base64'), iter);
    const ck = hmac(salted, Buffer.from('Client Key'));
    const proof = xor(ck, hmac(sha1(ck), Buffer.from(authMsg)));
    st.phase = 2;
    send("<response xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>" + b64(woProof + ',p=' + proof.toString('base64')) + '</response>');
    return;
  }
  if (st.phase === 2 && /<success/.test(d)) {
    st.phase = 3;
    send('<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" to="' + DOMAIN + '" version="1.0"/>');
    return;
  }
  if (st.phase === 2 && /<failure/.test(d)) return finish('SASL_FAIL', null, 2);
  if (st.phase === 3 && d.indexOf('stream:features') >= 0) {
    st.needSession = /<session/.test(d);
    st.phase = 4;
    send("<iq type='set' id='_bind_auth_2'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><resource>" + RESOURCE + '</resource></bind></iq>');
    return;
  }
  if (st.phase === 4 && /_bind_auth_2/.test(d)) {
    if (st.needSession) {
      st.phase = 45;
      send("<iq type='set' id='_session_auth_2'><session xmlns='urn:ietf:params:xml:ns:xmpp-session'/></iq>");
    } else queueConv();
    return;
  }
  if (st.phase === 45 && /_session_auth_2/.test(d)) { queueConv(); return; }
  if (st.phase === 5 && /<iq/.test(d) && /g_conv/.test(d)) {
    const parsed = parseIQBody(d);
    const cv = (parsed && parsed.conversations) || [];
    // 在翻历史 / 服务端清零之前先落快照；本脚本从不翻历史、从不发 history IQ
    const items = writeSnapshot(cv);
    refreshWx();
    refreshWidget(items);
    finish('OK', items, 0);
  }
}

appendLog('start resource=' + RESOURCE + ' once=1 service=' + SERVICE);
ws = new WebSocket(SERVICE, 'xmpp');
ws.onopen = () => send('<open xmlns="urn:ietf:params:xml:ns:xmpp-framing" to="' + DOMAIN + '" version="1.0"/>');
ws.onerror = e => { if (!done) finish('WSERR', null, 1); };
ws.onclose = e => { if (!done) finish('CLOSE_' + e.code, null, 1); };
ws.onmessage = ev => {
  const d = String(ev.data);
  try { handle(d); } catch (e) { finish('HERR', null, 1); }
};
hard = setTimeout(() => finish('TIMEOUT', null, 1), HARD_MS);
