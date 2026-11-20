// 桌面挂件生成器：从 项目数据看板.html 抽出 __DRAW，渲染成小尺寸挂件 HTML
// 用法: node research/build_widget.mjs   →  项目根目录/桌面挂件.html（ROOT 由脚本所在位置自动探测）
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const src = fs.readFileSync(ROOT + '/项目数据看板.html', 'utf8');

function extractAfter(marker) {
  const i = src.indexOf(marker);
  if (i < 0) return null;
  let j = i + marker.length, depth = 0, inStr = false, esc = false, start = j;
  for (; j < src.length; j++) {
    const c = src[j];
    if (inStr) { if (esc) esc = false; else if (c === '\\') esc = true; else if (c === '"') inStr = false; continue; }
    if (c === '"') { inStr = true; continue; }
    if (c === '{' || c === '[') depth++;
    else if (c === '}' || c === ']') { depth--; if (depth === 0) { return JSON.parse(src.slice(start, j + 1)); } }
  }
  return null;
}
const DRAW = extractAfter('window.__DRAW=');
if (!DRAW) { console.error('NO __DRAW'); process.exit(1); }
const DAYD = extractAfter('window.__DAYD=') || {};

const P = Object.values(DRAW);
const today = new Date();
const ymd = d => d.toISOString().slice(0, 10);
const pad = n => String(n).padStart(2, '0');
const TODAY = `${today.getFullYear()}-${pad(today.getMonth() + 1)}-${pad(today.getDate())}`;
const parseYmd = s => { const [y, m, d] = String(s).split('-').map(Number); return new Date(y, m - 1, d); };
const dayDiff = (a, b) => Math.round((parseYmd(a) - parseYmd(b)) / 86400000);
// 里程碑日期归一化：'2026-11-20' / '11/20' / Excel serial 都转成 YYYY-MM-DD
function toYmd(v) {
  if (v == null || v === '') return '';
  const s = String(v).trim();
  let m = s.match(/^(\d{4})-(\d{1,2})-(\d{1,2})$/);
  if (m) return `${m[1]}-${pad(+m[2])}-${pad(+m[3])}`;
  m = s.match(/^(\d{1,2})[\/\-](\d{1,2})$/);
  if (m) {
    const mo2 = +m[1], d2 = +m[2];
    let yy = today.getFullYear();
    if (mo2 < today.getMonth() + 1 - 5) yy += 1;
    return `${yy}-${pad(mo2)}-${pad(d2)}`;
  }
  if (/^\d{5}$/.test(s)) {
    const dt = new Date(Date.UTC(1899, 11, 30) + (+s) * 86400000);
    return `${dt.getUTCFullYear()}-${pad(dt.getUTCMonth() + 1)}-${pad(dt.getUTCDate())}`;
  }
  return '';
}

const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const shortModel = m => String(m || '').split(/[\/／]/)[0].trim();

// 未来 10 天节点
const nodes = [];
for (const p of P) for (const m of (p.ms || [])) {
  const d = toYmd(m.p);
  if (!d) continue;
  const dd = dayDiff(d, TODAY);
  if (dd < 0 || dd > 10) continue;
  nodes.push({ d, dd, model: shortModel(p.model), prio: p.prio, name: fixMojibake(m.n) });
}
nodes.sort((a, b) => (a.d < b.d ? -1 : a.d > b.d ? 1 : (a.prio < b.prio ? -1 : 1)));
// 同一机型只留最早 1 条，避免一个项目刷满整列
const seen = new Set();
const todo = nodes.filter(n => (seen.has(n.model) ? false : (seen.add(n.model), true))).slice(0, 5);

// 月历
const y = today.getFullYear(), mo = today.getMonth();
const first = new Date(y, mo, 1);
const startPad = (first.getDay() + 6) % 7; // 周一为首列
const daysIn = new Date(y, mo + 1, 0).getDate();
const perDay = {};
for (const p of P) for (const m of (p.ms || [])) { const d = toYmd(m.p); if (d) (perDay[d] = perDay[d] || []).push({ prio: p.prio, model: shortModel(p.model) }); }
let cells = '';
for (let i = 0; i < startPad; i++) cells += '<i class="cel out"></i>';
for (let d = 1; d <= daysIn; d++) {
  const key = `${y}-${pad(mo + 1)}-${pad(d)}`;
  const list = perDay[key] || [];
  const isT = key === TODAY;
  const p0 = list.some(x => x.prio === 'P0');
  const cls = 'cel' + (isT ? ' today' : '') + (list.length ? ' has' : '') + (p0 ? ' p0' : '');
  const tip = list.length ? `${key} ${list.map(x => x.model).join('、')}` : key;
  cells += `<i class="${cls}" title="${esc(tip)}"><b>${d}</b>${list.length ? '<u>' + '<s></s>'.repeat(Math.min(3, list.length)) + '</u>' : ''}</i>`;
}


// 全量节点（供挂件日历任意日期下钻）
const allNodes = [];
for (const p of P) for (const m of (p.ms || [])) {
  const d = toYmd(m.p); if (!d) continue;
  const dd = dayDiff(d, TODAY);
  if (dd < -7 || dd > 90) continue;
  allNodes.push({ d, dd, model: shortModel(p.model), prio: p.prio, name: m.n });
}

const p0s = P.filter(p => p.prio === 'P0');
const todoHtml = todo.map(n => {
  const badge = n.dd === 0 ? '今天' : (n.dd === 1 ? '明天' : `还有 ${n.dd} 天`);
  return `<li class="${n.dd <= 1 ? 'urgent' : ''}"><span class="dot ${n.prio.toLowerCase()}"></span><b>${esc(n.model)}</b><span class="nx">${esc(n.name)}</span><em>${badge}</em></li>`;
}).join('') || '<li class="empty">近 10 天无硬节点</li>';

const p0Html = p0s.slice(0, 3).map(p => `<div class="p0row"><i>P0</i><b>${esc(shortModel(p.model))}</b><span>${esc((p.blocker || '').slice(0, 42))}</span></div>`).join('');

const stamp = fs.statSync(ROOT + '/项目数据看板.html').mtime;
const hhmm = `${pad(stamp.getHours())}:${pad(stamp.getMinutes())}`;
const week = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][today.getDay()];

const html = `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<title>项目挂件</title><meta http-equiv="refresh" content="600">
<style>
:root{--bg:#0b0d12;--panel:rgba(18,20,26,.55);--line:rgba(255,255,255,.10);--ink:#eef1f6;--ink2:#a7aebd;--ink3:#6f7686;--acc:#ff9f43;--p0:#ff5f56;--p1:#ffbf47;--p2:#5aa9ff;--run:#2ecc8f}
*{box-sizing:border-box;margin:0;padding:0}
html,body{height:100%}
body{background:transparent;
 color:var(--ink);font:13px/1.5 "Segoe UI","Microsoft YaHei",system-ui,sans-serif;padding:16px 18px 12px;overflow:hidden;user-select:none}
.head{display:flex;align-items:flex-end;gap:12px;margin-bottom:12px}
.big{font-size:44px;font-weight:700;letter-spacing:-1px;line-height:.95;font-variant-numeric:tabular-nums}
.wk{font-size:12px;color:var(--ink2);padding-bottom:6px}
.chips{margin-left:auto;display:flex;gap:6px;align-items:center}
.chip{background:var(--panel);border:1px solid var(--line);border-radius:999px;padding:4px 10px;font-size:11px;color:var(--ink2);backdrop-filter:blur(10px)}
.chip b{color:var(--ink);font-weight:600}
.chip.acc{color:#1a1208;background:linear-gradient(180deg,#ffb968,#ff9f43);border-color:transparent;font-weight:700}
.grid{display:grid;grid-template-columns:minmax(0,1.05fr) minmax(0,.95fr);gap:12px}
.card{min-width:0}
.card{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:11px 12px;backdrop-filter:blur(14px);box-shadow:0 10px 30px rgba(0,0,0,.35)}
.ttl{font-size:11px;color:var(--ink3);letter-spacing:.08em;text-transform:uppercase;margin-bottom:7px;display:flex;align-items:center;gap:6px}
.ttl i{width:5px;height:5px;border-radius:50%;background:var(--acc);display:inline-block}
ul{list-style:none;display:flex;flex-direction:column;gap:5px}
li{display:grid;grid-template-columns:8px minmax(52px,auto) minmax(0,1fr) auto;align-items:center;gap:7px;font-size:12px;padding:3px 0;border-bottom:1px dashed rgba(255,255,255,.055)}
li:last-child{border-bottom:0}
li .dot{width:6px;height:6px;border-radius:50%;background:var(--p2)}
li .dot.p0{background:var(--p0)}li .dot.p1{background:var(--p1)}
li b{font-weight:600;font-family:"Cascadia Mono",Consolas,monospace;font-size:11.5px}
li .nx{color:var(--ink2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
li em{font-style:normal;font-size:10.5px;color:var(--ink3);background:rgba(255,255,255,.06);border-radius:5px;padding:1px 6px;white-space:nowrap}
li.urgent em{color:#1a1208;background:linear-gradient(180deg,#ffc271,#ff9f43);font-weight:700}
li.empty{grid-template-columns:1fr;color:var(--ink3);font-size:11.5px}
.cal{display:grid;grid-template-columns:repeat(7,1fr);gap:3px}
.wh{font-size:10px;color:var(--ink3);text-align:center;padding-bottom:2px}
.cel{position:relative;aspect-ratio:1/.92;border-radius:7px;display:flex;flex-direction:column;align-items:center;justify-content:center;font-size:11.5px;color:var(--ink2);border:1px solid transparent}
.cel.out{opacity:0}
.cel.has{background:rgba(255,255,255,.055);color:var(--ink)}
.cel.today{background:linear-gradient(180deg,rgba(255,159,67,.95),rgba(255,140,40,.9));color:#1a1208;font-weight:700;border-color:transparent}
.cel b{font-weight:inherit;font-variant-numeric:tabular-nums}
.cel u{display:flex;gap:2px;margin-top:2px;text-decoration:none}
.cel s{width:3px;height:3px;border-radius:50%;background:var(--p2);display:block}
.cel.p0 s:first-child{background:var(--p0)}
.cel.today s{background:#1a1208}
.p0box{margin-top:12px}
.p0row{display:grid;grid-template-columns:26px 92px 1fr;gap:8px;align-items:center;font-size:11.5px;padding:4px 0;border-bottom:1px dashed rgba(255,255,255,.055)}
.p0row:last-child{border-bottom:0}
.p0row i{font-style:normal;font-size:9.5px;font-weight:700;color:#fff;background:var(--p0);border-radius:4px;text-align:center;padding:1px 0}
.p0row b{font-family:"Cascadia Mono",Consolas,monospace;font-size:11.5px}
.p0row span{color:var(--ink2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.foot{margin-top:10px;display:flex;justify-content:space-between;font-size:10.5px;color:var(--ink3)}
</style></head><body>
<div class="head">
  <div class="big">${today.getDate()}</div>
  <div class="wk">${week} · ${y} 年 ${mo + 1} 月</div>
  <div class="chips">
    <span class="chip">在管 <b>${P.length}</b></span>
    <span class="chip acc">P0 <b>${p0s.length}</b></span>
    <span class="chip">近7天节点 <b>${nodes.filter(n => n.dd <= 7).length}</b></span>
  </div>
</div>
<div class="grid">
  <div class="card">
    <div class="ttl"><i></i>ToDo · 未来 10 天</div>
    <ul>${todoHtml}</ul>
    <div class="p0box">
      <div class="ttl"><i></i>P0 卡点</div>
      ${p0Html || '<div class="empty" style="color:var(--ink3);font-size:11.5px">无</div>'}
    </div>
  </div>
  <div class="card">
    <div class="ttl"><i></i>${mo + 1} 月节点</div>
    <div class="cal">${['一', '二', '三', '四', '五', '六', '日'].map(w => `<div class="wh">${w}</div>`).join('')}${cells}</div>
  </div>
</div>
<div class="foot"><span>数据 ${stamp.getFullYear()}-${pad(stamp.getMonth() + 1)}-${pad(stamp.getDate())} ${hhmm}</span><span>每 10 分钟自动刷新</span></div>
</body></html>`;

fs.writeFileSync(ROOT + '/桌面挂件.html', html, 'utf8');

// ---------- 今日信号（挂件「今日群聊信号」区，最多 3 条） ----------
const MODEL_TOKEN = /[A-Za-z]{2}\d{4,6}[A-Za-z]?/g;
const SIG_KW = /打样|工装|投模|开模|批量生产|交期|报价|核价|备料|订单|样机|试制|认证|包装|口味|烟油|丝印|喷油|成型|料号|BOM|图纸|规格|口数|电芯|客户|催|确认|结构|模评|立项|排期|产能|出货|验厂|寄样|样品/;
const SIG_TEXT_OK = t => t && t.length >= 8 && !/^\[图片|^\[文件|^\[语音|^\[视频/.test(t);
// 未读消息（群 + 私聊）：来自 E-Mobile 会话列表的 unreadcount，读过的自然消失
// 有些落盘文件是「UTF-8 字节被按 latin1 解码」的乱码，这里按需还原一次
function fixMojibake(str) {
  const s = String(str || '');
  if (!/[\u0080-\u00FF]{2,}/.test(s)) return s;
  try {
    const back = Buffer.from(s, 'latin1').toString('utf8');
    // 还原后若出现大量 U+FFFD 说明不是这个错误类型，回退原文
    const bad = (back.match(/\uFFFD/g) || []).length;
    return bad > back.length / 8 ? s : back;
  } catch (e) { return s; }
}

function myUid() {
  try {
    const d = JSON.parse(fs.readFileSync(ROOT + '/research/raw/emobile/api/msgconn.json', 'utf8'));
    const v = d.msg_user_id || d.uid || d.base_user_id || '';
    return String(v || '').trim();
  } catch (e) { return ''; }
}

function unreadSignals() {
  // 只认真未读：快照在每轮抓取「拉历史之前」采集（拉取会把服务端未读清零）
  // 每次运行都重读快照 → 纯 -NoOpen 渲染轮也会跟着刷新 signals，不依赖抓取轮
  let d = null;
  try { d = JSON.parse(fs.readFileSync(ROOT + '/research/raw/emobile/unread_snapshot.json', 'utf8')); } catch (e) { return []; }
  const arr = (d && d.items) || [];
  const me = myUid();
  const now = Date.now();
  // 同一会话可能重复推送/改名：按 convKey 或 targettype:targetid 去重，同 key 只留最新一条
  // 用稳定会话键而非群名 → 同名群、改名不会误合并成一条
  const byConv = new Map();
  for (let i = 0; i < arr.length; i++) {
    const c = arr[i];
    if (String(c.isdel) === '1') continue;                 // 已删除会话不参与去重
    let key = c.convKey || (String(c.targettype) + ':' + String(c.targetid));
    // 老快照既无 convKey 又无 targetid：加唯一后缀，避免把不同会话全折叠成一条
    if (!c.convKey && c.targetid == null) key += '|' + (c.msgid != null ? String(c.msgid) : String(i));
    const prev = byConv.get(key);
    if (!prev || Number(c.lasttime || 0) >= Number(prev.lasttime || 0)) byConv.set(key, c);
  }
  const rows = [];
  for (const c of byConv.values()) {
    const ts = Number(c.lasttime || 0);
    if (!ts || now - ts > 48 * 3600 * 1000) continue;
    // 最后一条是我发的 → 已经回过了，不算「待回复/未读」
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
      sender: name + (isPrivate ? '·私聊' : ''),
      text: (n > 0 ? n + '条未读 · ' : '') + (body.length > 52 ? body.slice(0, 52) + '…' : body),
      model: models[0] || '',
      ts, unread: n > 0 ? 1 : 0, src: 'EM', kind: isPrivate ? 'direct' : 'group', conv: name
    });
  }
  // 未读优先，其次按时间倒序；最多 3 条（挂件只有 3 行）
  rows.sort((a, b) => (b.unread - a.unread) || (b.ts - a.ts));
  return rows.slice(0, 3).map(s => ({ t: s.t, sender: s.sender, text: s.text, model: s.model }));
}

// 企微未读（只读本地库导出的快照）：与 E-Mobile 一起进未读面板
// 单位统一为毫秒：企微侧 ts 是秒，必须 ×1000，否则会被当成 1970 年而过滤掉
function wxSignals() {
  let d = null;
  try { d = JSON.parse(fs.readFileSync(ROOT + '/research/wxwork_unread.json', 'utf8')); } catch (e) { return []; }
  const arr = (d && (d.rows || d.items)) || [];
  const out = [];
  for (const r of arr) {
    let ts = Number(r.ts || 0);
    if (ts && ts < 1e12) ts *= 1000;                       // 秒 → 毫秒
    if (!ts) ts = Date.now();
    if (Date.now() - ts > 48 * 3600 * 1000) continue;
    const body = String(r.text || r.body || '').replace(/\s+/g, ' ').trim();
    if (body.length < 2) continue;
    const conv = String(r.conv || r.name || '').trim();
    const kind = String(r.kind || (String(r.conv || '').startsWith('S:') ? 'direct' : 'group'));
    out.push({
      t: String(new Date(ts).getHours()).padStart(2, '0') + ':' + String(new Date(ts).getMinutes()).padStart(2, '0'),
      sender: String(r.sender || '企微'),
      text: body.length > 64 ? body.slice(0, 64) + '…' : body,
      model: '',
      ts, unread: 1, src: '企微', kind, conv
    });
  }
  // 降级/过期要说出来：否则「面板 0 条未读」既可能是真没有，也可能是库没解出来
  const atStr = String((d && d.at) || '');
  const atMs = atStr ? Date.parse(atStr.replace(' ', 'T') + '+08:00') : 0;
  const stale = !atMs || (Date.now() - atMs > 5 * 60 * 1000);
  if ((d && d.degraded) || stale) {
    out.push({
      t: '--:--', sender: '企微',
      text: (d && d.degraded)
        ? ('未读快照降级（' + (d.reason || '解密/登录异常') + '），下面可能是上一轮数据')
        : ('未读快照已过期（' + atStr + '），未读面板可能没在更新'),
      model: '', ts: Date.now(), unread: 1, src: '企微', kind: 'group', conv: '未读状态'
    });
  }
  return out;
}

const signals = unreadSignals().concat(wxSignals())
  .sort((a, b) => (b.unread - a.unread) || (b.ts - a.ts))
  .slice(0, 3)
  .map(s => ({ t: s.t, sender: s.sender, text: s.text, model: s.model, src: s.src, kind: s.kind, conv: s.conv }));

// —— 运行健康度：给挂件画「绿/黄/红」联通状态灯用（只读已有产物，不额外发请求）
function healthBlock() {
  const rj = f => { try { return JSON.parse(fs.readFileSync(ROOT + '/research/' + f, 'utf8')); } catch (e) { return null; } };
  const es = rj('raw/emobile/em_full_pull_stats.json') || {};
  const mc = rj('raw/emobile/api/msgconn.json') || {};
  const it = rj('数据完整性.json') || {};
  const br = rj('brief_run_status.json') || {};
  const rt = rj('raw/emobile/realtime.json') || {};
  // 时间串必须验 NaN：老产物 / 手工改写可能写成本地无时区串，new Date() 得到 Invalid Date
  // 会让 ageMin 失真（审核 2026-11-14 防御项）。
  const atRaw = es.at ? new Date(es.at) : null;
  const at = atRaw && !Number.isNaN(atRaw.getTime()) ? atRaw : null;
  const ageMin = at ? Math.max(0, Math.round((Date.now() - at.getTime()) / 60000)) : -1;
  const wc = rj('widget_config.json') || {};
  const hr = new Date().getHours();
  const intervalMin = Number((hr >= 22 || hr < 8) ? (wc.nightMinutes || 60) : (wc.dayMinutes || 30));
  const steps = Array.isArray(br.steps) ? br.steps : [];
  // brief 的 Add-Step 现在输出 state/failed/skip 三态；旧版只把 skip 记 ok=true，
  // 单看 s.ok!==false 会漏掉「本该失败却被标成跳过」。显式认 failed / state==='fail'。
  const stepFailed = s => s && (s.failed === true || s.state === 'fail' || s.ok === false);
  const failedSteps = steps.filter(stepFailed).map(s => String(s.name || '?'));
  const skippedSteps = steps.filter(s => s && (s.skip === true || s.state === 'skip')).map(s => String(s.name || '?'));
  const runOk = steps.length ? failedSteps.length === 0 : Number(br.totalSec || 0) > 0;
  const msgs = Number(es.totalMessages || 0);
  // 真认证数据：msg_token 非空且抓取原因不是错误/断连；老产物没有 msg_token 时用 runOk && msgs>0 兜底，避免状态灯全红
  const token = String(mc.msg_token == null ? '' : mc.msg_token).trim();
  const reason = String(es.reason == null ? '' : es.reason).trim().toUpperCase();
  const reasonBad = reason === 'ERROR' || reason === 'WSERR' || reason === 'CLOSE';
  const authOk = token ? !reasonBad : (runOk && msgs > 0);
  const authAt = mc.at || '';
  // SCRAM 握手成功标记。旧版在字段缺失时静默兜底成 true，等于「无成功证据也算成功」。
  // 现在仍允许推定绿（避免老产物全红），但必须同时 runOk && authOk && msgs>0，
  // 并显式回传 scramInferred / scramSource，排障时一眼看出是推定而非实测。
  const scramExplicit = Object.prototype.hasOwnProperty.call(es, 'saslSuccess');
  const scramInferred = !scramExplicit;
  const scramOk = es.saslSuccess === true || (scramInferred && runOk && authOk && msgs > 0);
  const scramSource = scramExplicit ? 'explicit' : (scramOk ? 'inferred' : 'unknown');
  return {
    emobile: {
      host: mc.ws_host || '', port: Number(mc.ws_port || 7070),
      at: at ? at.toISOString() : '', ageMin, msgs,
      groups: Number(es.groups || 0), personal: Number(es.personalPulled || 0),
      // 实时在线状态（em_realtime.js 常驻进程写的 realtime.json）
      live: !!rt.connected && !!rt.at && (Date.now() - new Date(rt.at).getTime() < 120000),
      liveAt: rt.at || '', liveSince: rt.since || '', liveMsgs: Number(rt.messages || 0),
      liveLastMsgAt: rt.lastMsgAt || '', liveErr: rt.lastErr || '',
      intervalMin, runOk, lastAt: br.started || '',
      failedSteps, skippedSteps,
      authOk, authAt, scramOk, scramInferred, scramSource
    },
    wx: { at: it.at || '', sheets: Number(it.sheets || 0), lines: Number(it.httpLines || 0) },
    run: { started: br.started || '', totalSec: Number(br.totalSec || 0), fresh: !!br.fresh },
    alerts: Array.isArray(it.alerts) ? it.alerts.slice(0, 3) : []
  };
}

// 同时导出结构化数据，供 WPF 透明挂件使用
const data = {
  today: TODAY,
  todayDay: today.getDate(),
  week: week,
  ym: `${y} 年 ${mo + 1} 月`,
  stamp: `${stamp.getFullYear()}-${pad(stamp.getMonth() + 1)}-${pad(stamp.getDate())} ${hhmm}`,
  counts: { projects: P.length, p0: p0s.length, nodes10: nodes.length, nodes7: nodes.filter(n => n.dd <= 7).length },
  todo: todo.map(n => ({ model: n.model, name: n.name, prio: n.prio, dd: n.dd, d: n.d })),
  p0: p0s.slice(0, 4).map(p => ({ model: shortModel(p.model), blocker: (p.blocker || '').length > 64 ? (p.blocker || '').slice(0, 64) + '…' : (p.blocker || '') })),
  nodes: allNodes,
  signals,
  signalsMode: signals.length ? 'unread' : 'empty',
  health: healthBlock(),
  cal: { y, mo: mo + 1, startPad, daysIn, perDay: Object.fromEntries(Object.entries(perDay).map(([k, v]) => [k, v.map(x => x.prio)])) }
};
// 原子写：挂件随时可能读这个文件，半写会被 serde default 吞成空白看板
const _wTmp = ROOT + '/research/widget_data.json.tmp';
fs.writeFileSync(_wTmp, JSON.stringify(data), 'utf8');
fs.renameSync(_wTmp, ROOT + '/research/widget_data.json');
console.log('WIDGET OK | projects=' + P.length + ' nodes10=' + nodes.length + ' bytes=' + Buffer.byteLength(html));
