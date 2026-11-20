// CLIENT_A 日报发送：上传媒体 -> 共享给收件人 -> WS 发图片 + 文本
// 用法: node em_send_brief.js --peer <uid> --image <png> --thumb <jpg> --text "<文本>" [--dry-run]
//       node em_send_brief.js ... --confirm <token>   # 真发：必须带用户确认后生成的审批 token
// 说明: 不调用 /passport/login、不调 /msg/gettoken；access_token 从进程内存取（em_access_from_memory.js）
// 硬闸门: 任何非 --dry-run 发送，都必须先通过 research/client_send_approval.json 的审批校验
//         （approved=true / consumed=false / date=今天 / --confirm 与 token 一致 / text 与预览一致），
//         且校验发生在 liveToken()/upload()/WebSocket 之前 —— 未通过绝不联网。审批文件只由确认会话写，
//         本脚本只读 + 发送成功后置 consumed=true，绝不自行创建或置 approved=true（防绕过、防重放）。
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawnSync } = require('child_process');
// ws 延迟到「审批通过、真正要发」时才 require：dry-run 与审批未通过路径连模块都不加载，
// 保证硬闸门之前不存在任何网络依赖（NODE_PATH 未配时也能安全 dry-run / 拒绝发送）。

const DIR = __dirname;
const HOST = 'http://<LAN_HOST>:8999';
const argv = process.argv.slice(2);
const arg = (k, d) => { const i = argv.indexOf('--' + k); return i >= 0 ? argv[i + 1] : d; };
const has = k => argv.includes('--' + k);

const PEER = arg('peer', '169');
const IMAGE = arg('image', '');
const THUMB = arg('thumb', '');
const TEXT = arg('text', 'CLIENT_A 项目进度已更新');
const DRY = has('dry-run');
const CONFIRM = arg('confirm', '');

function log(o) { console.log(JSON.stringify(o)); }

// 审批文件路径固定在 research/client_send_approval.json（从 raw/emobile 上溯两级）。
// 只由用户在确认会话里批准（approved=true / consumed=false / date=今天 / token 与预览一致），
// 本脚本只读并在发送成功后置 consumed=true，绝不创建、绝不置 approved=true。
function approvalPath() { return path.join(DIR, '..', '..', 'client_send_approval.json'); }
function todayStr() {
  const d = new Date(), p = n => String(n).padStart(2, '0');
  return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate());
}
// 硬闸门：任何非 dry-run 调用都必须先过这里；未通过直接抛错，绝不进入 liveToken/upload/WebSocket。
function checkApproval() {
  const p = approvalPath();
  let ap;
  try { ap = JSON.parse(fs.readFileSync(p, 'utf8')); }
  catch (e) { throw new Error('SEND_NOT_APPROVED: missing/invalid approval file'); }
  if (ap.approved !== true) throw new Error('SEND_NOT_APPROVED: approved!=true (等待用户确认 OK)');
  if (ap.consumed === true) throw new Error('SEND_ALREADY_CONSUMED: 本次审批已用过，需重新预览确认');
  if (ap.date !== todayStr()) throw new Error('SEND_APPROVAL_EXPIRED: ' + ap.date + ' != ' + todayStr());
  if (ap.peer && String(ap.peer) !== String(PEER)) throw new Error('SEND_PEER_MISMATCH');
  if (!CONFIRM || CONFIRM !== String(ap.token)) throw new Error('SEND_NOT_APPROVED: --confirm token 与审批文件不一致');
  if (ap.text && ap.text !== TEXT) throw new Error('SEND_TEXT_MISMATCH: 发送文本与已确认的预览不一致');
  if (ap.imageSha256 && IMAGE) {
    const h = crypto.createHash('sha256').update(fs.readFileSync(IMAGE)).digest('hex');
    if (h !== ap.imageSha256) throw new Error('SEND_IMAGE_MISMATCH: 图片自预览后已变化，请重新预览确认');
  }
  return p;
}

// 发送成功后把审批一次性消费掉（防重放）。原子写：tmp -> rename，失败回退直接写。
function apSetConsumed(p) {
  const ap = JSON.parse(fs.readFileSync(p, 'utf8'));
  ap.consumed = true;
  ap.consumedAt = new Date().toISOString();
  const tmp = p + '.tmp';
  const buf = JSON.stringify(ap, null, 2);
  try { fs.writeFileSync(tmp, buf, 'utf8'); fs.renameSync(tmp, p); }
  catch (e) { try { fs.writeFileSync(p, buf, 'utf8'); } catch (e2) {} }
}

function liveToken() {
  const p = path.join(DIR, 'api', 'access_live.json');
  try {
    const j = JSON.parse(fs.readFileSync(p, 'utf8'));
    const tk = j.access_token || (j.data && j.data.access_token);
    const uid = String(j.msg_user_id || j.base_user_id || <UID>);
    const at = Number(j.at_ms || 0);
    if (tk && (!at || Date.now() - at < 6 * 3600 * 1000)) return { tk, uid, src: 'cache' };
  } catch (e) {}
  const r = spawnSync(process.execPath, [path.join(DIR, 'em_access_from_memory.js')], { encoding: 'utf8', timeout: 180000 });
  const line = (r.stdout || '').split('\n').filter(x => /VALID/.test(x)).pop() || '';
  try {
    const j = JSON.parse(fs.readFileSync(p, 'utf8'));
    const tk = j.access_token || (j.data && j.data.access_token);
    if (tk) return { tk, uid: String(j.msg_user_id || j.base_user_id || <UID>), src: 'memory' };
  } catch (e) {}
  throw new Error('NO_TOKEN ' + line.slice(0, 120));
}

async function upload(tk, uid, file) {
  const buf = fs.readFileSync(file);
  const fd = new FormData();
  fd.append('type', 'image');
  fd.append('from', uid);
  fd.append('media', new Blob([buf], { type: file.endsWith('.png') ? 'image/png' : 'image/jpeg' }), path.basename(file));
  const r = await fetch(HOST + '/emp/api/media/upload', {
    method: 'POST',
    headers: { access_token: tk, emaccesstk: tk, userheaderuserid: uid, emclienttag: 'em7' },
    body: fd,
  });
  const t = await r.text();
  let j = {}; try { j = JSON.parse(t); } catch (e) {}
  if (j.errcode !== 0 || !j.media_id) throw new Error('UPLOAD_FAIL ' + t.slice(0, 160));
  return j.media_id;
}

async function share(tk, uid, mid, peer) {
  const r = await fetch(HOST + '/emp/api/media/share', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', access_token: tk, emaccesstk: tk, userheaderuserid: uid, emclienttag: 'em7' },
    body: JSON.stringify({ media_ids: String(mid), touserids: String(peer) }),
  });
  const t = await r.text();
  let j = {}; try { j = JSON.parse(t); } catch (e) {}
  return { ok: j.errcode === 0, raw: t.slice(0, 120) };
}

function thumbJpeg(src, out) {
  if (THUMB && fs.existsSync(THUMB)) return THUMB;
  const py = '<TOOLS>/ppt-master-venv/Scripts/python.exe';
  const code = [
    'import sys',
    'from PIL import Image',
    'src,out=sys.argv[1],sys.argv[2]',
    'im=Image.open(src).convert("RGB")',
    'w=1080; h=int(im.height*w/im.width)',
    'im=im.resize((w,h), Image.LANCZOS)',
    'q=82',
    'while q>=45:',
    '    im.save(out,"JPEG",quality=q,optimize=True)',
    '    import os',
    '    if os.path.getsize(out)<=250000: break',
    '    q-=8',
  ].join('\n');
  const r = spawnSync(py, ['-c', code, src, out], { encoding: 'utf8', timeout: 120000 });
  if (!fs.existsSync(out)) throw new Error('THUMB_FAIL ' + (r.stderr || '').slice(0, 160));
  return out;
}

async function main() {
  if (IMAGE && !fs.existsSync(IMAGE)) throw new Error('NO_IMAGE ' + IMAGE);
  // 硬闸门必须在任何 token 读取/网络动作之前；dry-run 不读 token、不联网。
  let approvalFile = null;
  if (!DRY) approvalFile = checkApproval();
  if (DRY) {
    const thumb = THUMB || '(auto-thumb)';
    let gate = 'none';
    try {
      const ap = JSON.parse(fs.readFileSync(approvalPath(), 'utf8'));
      gate = ap.approved === true && ap.consumed !== true && ap.date === todayStr() ? 'approved' : 'waiting';
    } catch (e) { gate = 'missing'; }
    log({ sendOk: true, dryRun: true, peer: PEER, image: IMAGE || null, thumb, text: TEXT, approval: gate, tokenSrc: 'dry-run' });
    return;
  }
  const { tk, uid, src } = liveToken();
  const tok = { tokenSrc: src, uid };
  const mid = IMAGE ? await upload(tk, uid, IMAGE) : null;
  let sh = mid ? await share(tk, uid, mid, PEER) : { ok: null, raw: 'no-image' };
  if (sh && sh.ok === false && /42001|其他设备登录/.test(String(sh.raw || ''))) {
    try {
      require('child_process').execFileSync(process.execPath, [path.join(DIR, 'em_access_from_memory.js')], { stdio: 'ignore', timeout: 900000 });
      const live2 = JSON.parse(fs.readFileSync(path.join(DIR, 'api', 'access_live.json'), 'utf8'));
      const tk2 = live2.access_token || live2.token || tk;
      sh = await share(tk2, uid, mid, PEER);
      log({ shareRefresh: true, shareOk: !!(sh && sh.ok), raw: String(sh && sh.raw || '').slice(0, 80) });
    } catch (e) {
      log({ shareRefresh: 'fail', err: String(e.message || e) });
    }
  }
  const thumbFile = thumbJpeg(IMAGE, path.join(path.dirname(IMAGE), '_send_thumb.jpg'));
  const b64 = fs.readFileSync(thumbFile).toString('base64');

  const conn = JSON.parse(fs.readFileSync(path.join(DIR, 'api', 'msgconn.json'), 'utf8'));
  const TOKEN = conn['msg_token'];
  const UID = String(conn.msg_user_id || uid);
  const UDID = conn.msg_udid || 'c';
  const DOMAIN = conn.msg_domain || 'weaver';
  const AUTH = `${UID}|${UDID}`;
  const RES = 'br' + crypto.randomBytes(3).toString('hex');
  const B64 = s => Buffer.from(s).toString('base64');

  const frames = [];
  const mkImg = () => {
    const id = 'im' + crypto.randomBytes(3).toString('hex');
    const TO = `${PEER}|${UDID}@${DOMAIN}`;
    const msgid = `${TO}/${RES}/${id}`;
    const extra = JSON.stringify({
      senderid: UID, msg_id: msgid, countids: crypto.randomUUID(), receiverids: PEER,
      issetted: '', msg_at_userid: '', thumbnailId: '', imgUri: '', imgUrl: mid,
    });
    const body = JSON.stringify({ imgUrl: '', extra, objectName: 'RC:ImgMsg', msgid, targetType: '0', content: b64 });
    return `<message id='${id}' to='${TO}' from='${AUTH}@${DOMAIN}/${RES}' type='chat'><body>${body.replace(/</g, '&lt;')}</body></message>`;
  };
  const mkTxt = () => {
    const id = 'tx' + crypto.randomBytes(3).toString('hex');
    const TO = `${PEER}|${UDID}@${DOMAIN}`;
    const msgid = `${TO}/${RES}/${id}`;
    const body = JSON.stringify({ objectName: 'RC:TxtMsg', msgid, content: TEXT, extra: JSON.stringify({}) });
    return `<message id='${id}' to='${TO}' from='${AUTH}@${DOMAIN}/${RES}' type='chat'><body>${body.replace(/</g, '&lt;')}</body></message>`;
  };

  // 到这里才加载 ws：已过审批闸门、确定真发，模块缺失只影响发送、不影响 dry-run/审批拒绝。
  const WebSocket = require('ws');
  const ws = new WebSocket(conn.ws_url, 'xmpp', { headers: { 'User-Agent': 'EM/7.0.14' } });
  let phase = 0, nonce;
  const done = (code, extra) => {
    // 只在真正发送成功时把审批置 consumed（一次性，防重放）；失败保留审批以便重试。
    let consumed = false;
    if (code === 0 && approvalFile) {
      try { apSetConsumed(approvalFile); consumed = true; } catch (e) { /* 不影响已发出的事实 */ }
    }
    log(Object.assign({ sendOk: code === 0, peer: PEER, mediaId: mid || null, shareOk: !!(sh && sh.ok), thumbBytes: thumbFile ? fs.statSync(thumbFile).size : 0, bytes: frames.reduce((a, f) => a + f.length, 0), frames: frames.length, approvalConsumed: consumed }, tok, extra || {}));
    setTimeout(() => { try { ws.close(); } catch (e) {} process.exit(code); }, 500);
  };
  ws.on('open', () => ws.send(`<open xmlns='urn:ietf:params:xml:ns:xmpp-framing' to='${DOMAIN}' version='1.0'/>`));
  ws.on('message', d => {
    const s = d.toString();
    if (phase === 0 && s.includes('stream:features')) {
      nonce = crypto.randomBytes(12).toString('hex');
      ws.send(`<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='SCRAM-SHA-1'>${B64('n,,' + `n=${AUTH},r=${nonce}`)}</auth>`);
      phase = 1; return;
    }
    if (phase === 1 && s.includes('<challenge')) {
      const raw = Buffer.from(s.split('>')[1].split('<')[0], 'base64').toString();
      const kv = Object.fromEntries(raw.split(',').map(x => [x[0], x.slice(2)]));
      const salted = crypto.pbkdf2Sync(TOKEN, Buffer.from(kv.s, 'base64'), parseInt(kv.i, 10), 20, 'sha1');
      const ck = crypto.createHmac('sha1', salted).update('Client Key').digest();
      const sk = crypto.createHash('sha1').update(ck).digest();
      const wp = `c=biws,r=${kv.r}`;
      const cbare = `n=${AUTH},r=${nonce}`;
      const sig = crypto.createHmac('sha1', sk).update(`${cbare},${raw},${wp}`).digest();
      const proof = Buffer.alloc(ck.length); for (let i = 0; i < ck.length; i++) proof[i] = ck[i] ^ sig[i];
      ws.send(`<response xmlns='urn:ietf:params:xml:ns:xmpp-sasl'>${B64(`${wp},p=${proof.toString('base64')}`)}</response>`);
      phase = 2; return;
    }
    if (phase === 2 && s.includes('<success')) { ws.send(`<open xmlns='urn:ietf:params:xml:ns:xmpp-framing' to='${DOMAIN}' version='1.0'/>`); phase = 3; return; }
    if (phase === 3 && s.includes('stream:features')) { ws.send(`<iq type='set' id='b1'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'><resource>${RES}</resource></bind></iq>`); phase = 4; return; }
    if (phase === 4 && /id=["']b1["']/.test(s)) { ws.send(`<iq type='set' id='s1'><session xmlns='urn:ietf:params:xml:ns:xmpp-session'/></iq>`); phase = 5; return; }
    if (phase === 5 && /id=["']s1["']/.test(s)) {
      phase = 6;
      if (IMAGE) {
        const f1 = mkImg(); frames.push(f1); ws.send(f1);
        setTimeout(() => { const f2 = mkTxt(); frames.push(f2); ws.send(f2); }, 700);
        setTimeout(() => done(0), 6000);
      } else {
        const f2 = mkTxt(); frames.push(f2); ws.send(f2);
        setTimeout(() => done(0), 2500);
      }
    }
  });
  ws.on('error', e => { done(1, { err: e.message }); });
  setTimeout(() => { if (phase < 6) done(2, { err: 'TIMEOUT_BIND' }); }, 45000);
}

main().catch(e => { log({ sendOk: false, err: String(e.message || e) }); process.exit(1); });
