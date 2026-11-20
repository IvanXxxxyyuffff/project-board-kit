#!/usr/bin/env node
/*
 * wxwork_sheet_pull.js —— 企业微信（WXWork 5.x）文档表格只读导出（纯 HTTP，无 UI / 无浏览器 / 无 OCR）
 *
 * 原理链：
 *   1) Restart Manager 问出「谁占着 Cookies 库」→ 拿到活着的 WXWorkWeb pid
 *   2) 以只读方式取一份 Cookies 库的副本（不改动原库）
 *      （wxwork_handle.exe grab，单进程内完成「定位 + 复制」，避免进程重启导致句柄失效）
 *   3) Local State 的 os_crypt.encrypted_key → DPAPI 解出 32B AES key
 *   4) v10 cookie → AES-256-GCM 解密 → 组装 Cookie 头
 *   5) https.request 打 dop-api/get/sheet，按动态发现的 subId 拉页签
 *   6) base64 + deflate 解压 → 宽松文本 harvest（与旧版浏览器脚本同一套规则）
 *
 * 用法：
 *   node wxwork_sheet_pull.js                 # 全量导出动态发现的全部页签
 *   node wxwork_sheet_pull.js --check         # 探活（每日管道用），打印 CHECK cookies=N ok=true
 *   node wxwork_sheet_pull.js --cookies       # 只导出 research/wxwork_cookies.json（playwright 格式）
 *   node wxwork_sheet_pull.js --padid1=e3_… --padid2=e3_…
 *   node wxwork_sheet_pull.js --verify        # 附带 WXWork 目录前后指纹比对
 *
 * 退出码：0 成功 / 1 未登录或拿不到 cookie 库 / 2 cookie 过期或无权限 / 3 全部页签失败
 *
 * 只读承诺：不写 WXWork 目录、不改注册表、不注入进程、不上传任何数据。
 */
'use strict';

const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const crypto = require('crypto');
const zlib = require('zlib');
const { spawnSync } = require('child_process');

// ---------------------------------------------------------------- 路径与常量
const ROOT = path.resolve(__dirname, '..', '..', '..');
const EXE = path.join(ROOT, 'research', 'wxwork_handle.exe');
const OUT_DIR = path.join(ROOT, 'research', 'wxdata');
const OUT_TXT = path.join(OUT_DIR, 'ALL_HTTP_TEXT_WXWORK.txt');
const OUT_STATS = path.join(OUT_DIR, 'wxwork_pull_stats.json');
const OUT_COOKIES = path.join(ROOT, 'research', 'wxwork_cookies.json');
const TMP = os.tmpdir();
const COOKIES_COPY = path.join(TMP, 'wxwork_cookies_copy.db');

const WX_DOCS = path.join(os.homedir(), 'Documents', 'WXWork');
const LOCAL_STATE = path.join(WX_DOCS, 'Local State');
// 账号目录按 /^\d{10,}$/ 动态发现，避免把某个账号 id 写死
const ACCOUNT_CEF_ROOTS = (() => {
  try {
    return fs.readdirSync(WX_DOCS, { withFileTypes: true })
      .filter(d => d.isDirectory() && /^\d{10,}$/.test(d.name))
      .map(d => path.join(WX_DOCS, d.name, 'WXWorkCefCache'));
  } catch (e) { return []; }
})();
const CEF_ROOTS = [path.join(WX_DOCS, 'qtCef'), ...ACCOUNT_CEF_ROOTS];

// 故意留空，页签靠 discoverSheets() 从 collab_client_vars.header[0].d[] 动态发现
// （静态页签清单会过期：页签被删/改名后 subId 失效 → retcode ITEM_X invalid sheetID）
const SHEETS = [];
SHEETS.forEach((s, i) => { s.file = s.doc + '_' + String(i + 1).padStart(2, '0') + '_' + s.id; });

// 真实 UA（取自 qtCef 的 Local Storage / IndexedDB 里文档页自身上报的 UA）
const UA = 'Mozilla/5.0 (Windows NT 6.2; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) ' +
  'Chrome/129.0.6668.101 Safari/537.36 Language/zh wxwork/5.0.13 (MicroMessenger/6.2) ' +
  'WindowsWechat MailPlugin_Electron WeMail embeddisk wxworkQt wxworkQtDocVersion/1';

const COOKIE_DOMAINS = ['.doc.weixin.qq.com', 'doc.weixin.qq.com', '.weixin.qq.com'];

// 已知 Cookies 库大小（CEF journal 模式下库大小稳定；逐个试，最后兜底 size=0 内容指纹）
const DB_SIZE_CANDIDATES = [28672, 32768, 36864, 24576, 20480, 16384, 0];

// ---------------------------------------------------------------- 参数
const argv = process.argv.slice(2);
const has = f => argv.includes(f);
const opt = (k) => { const p = argv.find(a => a.startsWith('--' + k + '=')); return p ? p.slice(k.length + 3) : null; };
const CHECK = has('--check');
const ONLY_COOKIES = has('--cookies');
const VERIFY = has('--verify');
const CONCURRENCY = Number(opt('concurrency') || 4);

const log = (...a) => console.error(...a);       // 进度走 stderr，stdout 只放结果行

// ---------------------------------------------------------------- 小工具
function runExe(args, timeout) {
  const r = spawnSync(EXE, args, { encoding: 'utf8', timeout: timeout || 60000, windowsHide: true });
  return { code: r.status, out: (r.stdout || '') + (r.stderr || '') };
}

let DatabaseSync = null;
try { ({ DatabaseSync } = require('node:sqlite')); } catch (e) { log('node:sqlite 不可用: ' + e.message); }

// ---------------------------------------------------------------- 1. 复制 Cookies 库
function findHolder() {
  // Restart Manager 只是拿「谁在占」用于日志/诊断，真正复制由 grab 自完成
  for (const root of CEF_ROOTS) {
    const p = path.join(root, 'Network', 'Cookies');
    if (!fs.existsSync(p)) continue;
    const r = runExe(['who', p], 30000);
    const m = /RM\|HOLDER\|pid=(\d+)\|app=([^|]*)/.exec(r.out);
    if (m) return { path: p, pid: Number(m[1]), app: m[2] };
  }
  return null;
}

function cookieDbsIn(dbPath) {
  if (!DatabaseSync || !fs.existsSync(dbPath)) return [];
  try {
    const db = new DatabaseSync(dbPath, { readOnly: true });
    const rows = db.prepare(
      'SELECT host_key, name, path, value, encrypted_value, CAST(expires_utc AS TEXT) exp, ' +
      'is_secure, is_httponly, samesite FROM cookies ORDER BY host_key, name'
    ).all();
    db.close();
    return rows;
  } catch (e) { return []; }
}

function grabCookiesDb() {
  if (!fs.existsSync(EXE)) throw new Error('缺少 ' + EXE + '（先编译 wxwork_handle.cs）');
  for (const size of DB_SIZE_CANDIDATES) {
    try { if (fs.existsSync(COOKIES_COPY)) fs.unlinkSync(COOKIES_COPY); } catch (e) { /* ignore */ }
    const r = runExe(['grab', String(size), COOKIES_COPY, '25000'], 45000);
    if (!fs.existsSync(COOKIES_COPY)) continue;
    const rows = cookieDbsIn(COOKIES_COPY);
    if (rows.length && rows.some(x => /weixin\.qq\.com$/.test(x.host_key))) {
      log(`[grab] size=${size} rows=${rows.length} → ${COOKIES_COPY}`);
      return rows;
    }
  }
  return [];
}

// ---------------------------------------------------------------- 2. 主密钥 + 解密
function masterKey() {
  const ls = JSON.parse(fs.readFileSync(LOCAL_STATE, 'utf8'));
  const b64 = ls.os_crypt && ls.os_crypt.encrypted_key;
  if (!b64) throw new Error('Local State 里没有 os_crypt.encrypted_key');
  const r = runExe(['dpapi', b64], 30000);
  const m = /hex=([0-9a-fA-F]+)/.exec(r.out);
  if (!m) throw new Error('DPAPI 解密失败: ' + r.out.trim());
  const k = Buffer.from(m[1], 'hex');
  if (k.length !== 32) throw new Error('主密钥长度异常: ' + k.length);
  return k;
}

function decryptValue(u8, key) {
  if (!u8 || !u8.length) return '';
  const buf = Buffer.from(u8);
  const pfx = buf.subarray(0, 3).toString('latin1');
  if (pfx !== 'v10' && pfx !== 'v11') return buf.toString('utf8');   // 明文
  if (pfx === 'v20') return '';                                       // App-Bound 加密，当前不支持
  const d = crypto.createDecipheriv('aes-256-gcm', key, buf.subarray(3, 15));
  d.setAuthTag(buf.subarray(buf.length - 16));
  return Buffer.concat([d.update(buf.subarray(15, buf.length - 16)), d.final()]).toString('utf8');
}

function readCookies(rows, key) {
  const now = Date.now();
  const out = [];
  for (const r of rows) {
    try {
      const val = (typeof r.value === 'string' && r.value) ? r.value : decryptValue(r.encrypted_value, key);
      if (!val) continue;
      const expMs = Number(r.exp) / 1000 - 11644473600000;
      if (expMs > 0 && expMs < now) continue;                          // 过期
      out.push({
        name: r.name, value: val, domain: r.host_key, path: r.path || '/',
        expires: expMs > 0 ? Math.floor(expMs / 1000) : -1,
        httpOnly: !!r.is_httponly, secure: !!r.is_secure,
        sameSite: r.samesite === 1 ? 'Lax' : (r.samesite === 2 ? 'Strict' : 'None')
      });
    } catch (e) { /* 单条解密失败不影响整体 */ }
  }
  return out;
}

// ---------------------------------------------------------------- 3. padId 解析
function padIdFromCapture(file) {
  try {
    const s = fs.readFileSync(file, 'utf8');
    const m = /"padId":"([^"]+)"/.exec(s);
    return m ? m[1] : null;
  } catch (e) { return null; }
}

function resolvePadIds() {
  let id1 = opt('padid1'), id2 = opt('padid2');
  if (!id1) id1 = padIdFromCapture(path.join(OUT_DIR, 'LIVE_T1_opendoc.json'));
  if (!id2) id2 = padIdFromCapture(path.join(OUT_DIR, 'LIVE_T2_opendoc.json'));
  if (!id1 || !id2) {
    const cfg = path.join(OUT_DIR, 'doc_ids.json');
    if (fs.existsSync(cfg)) {
      try { const j = JSON.parse(fs.readFileSync(cfg, 'utf8')); id1 = id1 || j.padId1; id2 = id2 || j.padId2; } catch (e) { }
    }
  }
  return { T1: id1, T2: id2 };
}

// ---------------------------------------------------------------- 4. HTTP
function httpGet(url, cookieHeader) {
  return new Promise(resolve => {
    const u = new URL(url);
    const t0 = Date.now();
    const req = https.request({
      hostname: u.hostname, path: u.pathname + u.search, method: 'GET',
      headers: {
        'Cookie': cookieHeader, 'User-Agent': UA,
        'Referer': 'https://doc.weixin.qq.com/',
        'Accept': 'application/json, text/plain, */*',
        'Accept-Language': 'zh-CN,zh;q=0.9',
        'Connection': 'keep-alive'
      }
    }, res => {
      const ch = [];
      res.on('data', c => ch.push(c));
      res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(ch), ms: Date.now() - t0 }));
    });
    req.setTimeout(30000, () => req.destroy(new Error('timeout')));
    req.on('error', e => resolve({ status: 0, body: Buffer.alloc(0), ms: Date.now() - t0, error: String(e.message) }));
    req.end();
  });
}

// ---------------------------------------------------------------- 5. harvest（与旧版浏览器脚本同规则）
const NOISE = /^(微软雅黑|宋体|等线|Arial|Calibri|General|Times New Roman|黑体|仿宋|等线 Light|_|\*)$/;
const RE = /[\u4e00-\u9fa5A-Za-z0-9][\u4e00-\u9fa5A-Za-z0-9 \/\-（）()：:.,，。、；;×xXmMlL%#'"_\[\]]{1,180}/g;

function harvest(text, set) {
  let m;
  RE.lastIndex = 0;
  while ((m = RE.exec(text)) !== null) {
    const s = m[0].replace(/\s{2,}/g, ' ').trim();
    if (s.length < 2 || NOISE.test(s)) continue;
    if (!/[\u4e00-\u9fa5A-Za-z0-9]/.test(s)) continue;
    set.add(s);
  }
}

function inflate(buf) {
  for (const fn of [zlib.inflateRawSync, zlib.inflateSync, zlib.brotliDecompressSync]) {
    try { return fn(buf); } catch (e) { /* next */ }
  }
  return null;
}

function stringsOfSheet(json) {
  const set = new Set();
  const data = json && json.data;
  if (!data) return [];
  const texts = (data.initialAttributedText && data.initialAttributedText.text) || [];
  for (const t of texts) {
    for (const key of ['workbook', 'related_sheet']) {
      const b64 = t[key];
      if (typeof b64 !== 'string' || b64.length < 32) continue;
      const raw = Buffer.from(b64, 'base64');
      const plain = inflate(raw);
      if (plain) harvest(plain.toString('utf8'), set);
      else harvest(raw.toString('latin1'), set);        // 兜底：未压缩就往原文扫
    }
  }
  return Array.from(set);
}

// ---------------------------------------------------------------- 5.5 页签动态发现
// 静态清单会过期（页签被删/改名，subId 失效 → retcode ITEM_X invalid sheetID）。
// 真实页签列表在文档元数据里：data.collab_client_vars.header[0].d[]（type=grid）。
function extractHeader(json) {
  if (!json) return [];
  const cands = [];
  for (const src of [json, json.data, json.clientVars]) {
    if (!src) continue;
    const ccv = src.collab_client_vars;
    if (ccv && ccv.header) for (const h of ccv.header) if (h && h.d) cands.push(h.d);
    if (Array.isArray(src.header)) cands.push(src.header);
    if (src.header && Array.isArray(src.header.d)) cands.push(src.header.d);
  }
  for (const arr of cands) {
    const sheets = arr.filter(x => x && x.id)
      .map(x => ({ id: String(x.id), name: String(x.name || '').trim() }));
    if (sheets.length) return sheets;
  }
  return [];
}

async function discoverSheets(padId, cookieHeader, seedSub) {
  const urls = [
    'https://doc.weixin.qq.com/dop-api/get/sheet?padId=' + encodeURIComponent(padId) +
    '&subId=' + encodeURIComponent(seedSub) + '&startrow=0&xsrf=&outformat=1&normal=1&preview_token=',
    'https://doc.weixin.qq.com/dop-api/opendoc?padId=' + encodeURIComponent(padId) +
    '&normal=1&outformat=1&xsrf=&preview_token='
  ];
  for (const url of urls) {
    const r = await httpGet(url, cookieHeader);
    if (r.status !== 200 || !r.body.length) continue;
    try {
      const list = extractHeader(JSON.parse(r.body.toString('utf8')));
      if (list.length) return list;
    } catch (e) { /* next */ }
  }
  return [];
}

// ---------------------------------------------------------------- 6. 场景：--check / --cookies / 全量
async function acquire() {
  const holder = findHolder();
  if (holder) log(`[holder] pid=${holder.pid} app=${holder.app}`);
  const rows = grabCookiesDb();
  if (!rows.length) return { err: 'NO_DB' };
  const key = masterKey();
  const cookies = readCookies(rows, key);
  const docCookies = cookies.filter(c => COOKIE_DOMAINS.includes(c.domain));
  return { holder, cookies, docCookies };
}

function fingerprint(dir) {
  // 只对本次可能触及的文件取证（Cookie 库 + Local State），整树 hash 太慢
  const targets = [
    ...CEF_ROOTS.map(r => path.join(r, 'Network', 'Cookies')),
    path.join(WX_DOCS, 'Local State')
  ];
  const out = [];
  for (const p of targets) {
    const r = runExe(['hash', p], 60000);
    const line = (r.out || '').split(/\r?\n/).find(l => l.startsWith('HASH|')) || ('HASH|err|' + (r.status));
    out.push(p + ' => ' + line);
  }
  return crypto.createHash('sha256').update(out.join('\n')).digest('hex') + ' :: ' + out.join(' ;; ');
}

async function main() {
  const started = Date.now();
  let acq;
  try { acq = await acquire(); } catch (e) { log('ERROR ' + e.message); process.exit(1); }
  if (acq.err === 'NO_DB') {
    log('ERROR 拿不到 Cookies 库：WXWork 未运行 / 未登录 / 句柄复制失败');
    process.exit(1);
  }

  if (CHECK) {
    const ok = acq.docCookies.length > 0;
    console.log(`CHECK cookies=${acq.docCookies.length} ok=${ok}`);
    process.exit(ok ? 0 : 2);
  }

  if (ONLY_COOKIES) {
    const arr = acq.cookies.filter(c => COOKIE_DOMAINS.includes(c.domain));
    fs.writeFileSync(OUT_COOKIES, JSON.stringify(arr, null, 2), 'utf8');
    console.log(`COOKIES_WRITTEN n=${arr.length} path=${OUT_COOKIES}`);
    process.exit(arr.length ? 0 : 2);
  }

  const docCookies = acq.docCookies;
  if (!docCookies.length) {
    log('ERROR 没有 doc.weixin.qq.com 域 cookie（可能未登录文档）');
    process.exit(1);
  }
  const cookieHeader = docCookies.map(c => c.name + '=' + c.value).join('; ');

  const pads = resolvePadIds();
  if (!pads.T1 || !pads.T2) {
    log('ERROR 缺少 padId：请用 --padid1/--padid2 传入，或放置 research/wxdata/doc_ids.json');
    process.exit(1);
  }

  // 页签清单：动态发现优先；动态发现 <3 且无静态清单时明确报错退出，不允许 0 页签还报成功
  let plan = [];
  for (const docKey of ['T1', 'T2']) {
    const disc = await discoverSheets(pads[docKey], cookieHeader, '000002');
    if (disc.length >= 3) {
      log(`[discover] ${docKey} sheets=${disc.length}`);
      plan.push(...disc.map(s => ({ doc: docKey, id: s.id, name: s.name })));
    } else if (SHEETS.length) {
      log(`[discover] ${docKey} 失败（发现 ${disc.length} 个），退回静态清单`);
      plan.push(...SHEETS.filter(s => s.doc === docKey).map(s => ({ doc: s.doc, id: s.id, name: s.name })));
    } else {
      log(`ERROR ${docKey} 动态页签发现失败（仅发现 ${disc.length} 个，需 >=3），且 SHEETS 静态清单为空；拒绝以 0 页签继续`);
      process.exit(1);
    }
  }
  if (!plan.length) {
    log('ERROR 页签计划为空：动态发现与静态清单均不可用，拒绝静默抓 0 页签还报成功');
    process.exit(1);
  }
  plan.forEach((s, i) => { s.file = s.doc + '_' + String(i + 1).padStart(2, '0') + '_' + s.id; });

  const fpBefore = VERIFY ? fingerprint(WX_DOCS) : null;

  const results = [];
  let authFail = 0, hardFail = 0;
  const queue = plan.slice();
  const workers = new Array(Math.max(1, Math.min(CONCURRENCY, queue.length))).fill(0).map(async () => {
    while (queue.length) {
      const s = queue.shift();
      const padId = pads[s.doc];
      const url = 'https://doc.weixin.qq.com/dop-api/get/sheet?padId=' + encodeURIComponent(padId) +
        '&subId=' + s.id + '&startrow=0&xsrf=&outformat=1&normal=1&preview_token=';
      const r = await httpGet(url, cookieHeader);
      let json = null;
      if (r.status === 200 && r.body.length) { try { json = JSON.parse(r.body.toString('utf8')); } catch (e) { } }
      const rc = json ? json.retcode : -1;
      const strings = (json && rc === 0) ? stringsOfSheet(json) : [];
      if (rc !== 0) {
        const msg = (json && json.errmsg) || r.error || ('status=' + r.status);
        if (/login|auth|token|permission|expire|sid/i.test(String(msg))) authFail++; else hardFail++;
        log(`[fail] ${s.file} status=${r.status} retcode=${rc} ${String(msg).slice(0, 80)}`);
      } else {
        log(`[ok]   ${s.file} ${s.name} strings=${strings.length} bytes=${r.body.length} ms=${r.ms}`);
      }
      results.push({ sheet: s, rc, status: r.status, strings, bytes: r.body.length });
      await new Promise(res => setTimeout(res, 200));
    }
  });
  await Promise.all(workers);

  results.sort((a, b) => plan.indexOf(a.sheet) - plan.indexOf(b.sheet));
  const okSheets = results.filter(x => x.rc === 0);

  if (!okSheets.length) {
    log(`ERROR 全部页签失败（authFail=${authFail} hardFail=${hardFail}）`);
    process.exit(authFail ? 2 : 3);
  }

  // 落盘
  const parts = [];
  for (const x of results) {
    if (x.rc !== 0) continue;
    parts.push(`\n===== ${x.sheet.file} | ${x.sheet.name} =====\n`);
    x.strings.forEach((s, i) => {
      parts.push(String(i + 1).padStart(4, '0') + '  ' + s.replace(/[\r\n]+/g, ' ⏎ ') + '\n');
    });
  }
  const text = parts.join('');
  fs.mkdirSync(OUT_DIR, { recursive: true });
  fs.writeFileSync(OUT_TXT, text, 'utf8');

  const fpAfter = VERIFY ? fingerprint(WX_DOCS) : null;
  const stats = {
    at: new Date().toISOString(),
    cookiesFound: docCookies.length,
    expired: 0,
    sheets: okSheets.length,
    sheetsTotal: plan.length,
    bytes: Buffer.byteLength(text, 'utf8'),
    ms: Date.now() - started,
    holderPid: acq.holder ? acq.holder.pid : null,
    failed: results.filter(x => x.rc !== 0).map(x => ({ file: x.sheet.file, status: x.status, retcode: x.rc })),
    sheetsList: plan.map(s => ({ doc: s.doc, id: s.id, name: s.name })),
    fingerprintMatch: fpBefore && fpAfter ? fpBefore === fpAfter : null
  };
  fs.writeFileSync(OUT_STATS, JSON.stringify(stats, null, 2), 'utf8');

  console.log(`OK sheets=${stats.sheets}/${stats.sheetsTotal} bytes=${stats.bytes} ms=${stats.ms} ` +
    `cookies=${stats.cookiesFound} out=${OUT_TXT}`);
  if (stats.fingerprintMatch !== null) console.log(`FINGERPRINT match=${stats.fingerprintMatch}`);
}

main().catch(e => { log('FATAL ' + (e && e.stack || e)); process.exit(1); });
