// 纯 HTTP 拉取两张腾讯文档表的全部页签（依赖 research/wxwork_cookies.json）
// 用法:
//   node research/wxdoc_pull_all.mjs --selftest        # 用本地 LIVE_T1_opendoc.json 验证解码链路
//   node research/wxdoc_pull_all.mjs                   # 真实拉取 20 页签
import fs from 'node:fs';
import zlib from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

let ROOT = path.dirname(fileURLToPath(import.meta.url));
while (ROOT && !fs.existsSync(path.join(ROOT, '每日项目简报.ps1'))) {
  const parent = path.dirname(ROOT);
  if (!parent || parent === ROOT) break;
  ROOT = parent;
}
const RESEARCH = path.join(ROOT, 'research');
const DIR = path.join(RESEARCH, 'wxdata');
const CJ = path.join(RESEARCH, 'wxwork_cookies.json');
const OUT = path.join(DIR, 'ALL_HTTP_TEXT.txt');
const OUT_COPY = path.join(DIR, 'ALL_HTTP_TEXT_NODE.txt');
const BASE = 'https://doc.weixin.qq.com';

// T1 14 个页签 / T2 6 个页签（顺序与表格页签一致）
const T1 = [
  ['000002', '报表注释', true], ['000003', '项目成员', false], ['80kj0e', '延期分析表', false],
  ['hgh93y', '衍生品（不开模）-进度表', true], ['000004', '新项目开模-进度表', false], ['yhqwsm', '其它类型项目--进度表', false],
  ['6j8vec', 'ITEM_X(开模)', false], ['4qvvy8', 'ITEM_X（3D打印）', false], ['lff01b', 'ITEM_X', false],
  ['x2ngwb', 'ITEM_X', false], ['66yzh0', '汇总表-模版-模版V5', true], ['j3565j', 'ITEM_X', false],
  ['4ydg2w', 'ITEM_X', false], ['aywqyw', 'ITEM_X', false],
];
const T2 = [
  ['mm9do6', 'T2-01', false], ['cleajl', 'T2-02', false], ['000002', '项目成员（预立项）', false],
  ['0luzo9', 'T2-04', false], ['000003', '预立项-进度汇总表', false], ['nkaon4', 'T2-06', false],
];

function padIdOf(file) {
  try {
    const j = JSON.parse(fs.readFileSync(path.join(DIR, file), 'utf8'));
    const cv = j.clientVars || {};
    const bd = j.bodyData || {};
    const cand = cv.localPadId || bd.localPadId || cv.padId || bd.padId;
    if (cand && /^e3_/.test(cand)) return cand;
  } catch (e) { }
  return null;
}

// 宽松抽文本：把解压后的字节按 utf8 解，抽出长度>=2 的可读串
function harvest(buf) {
  const s = buf.toString('utf8');
  const out = [];
  const re = /[\u4e00-\u9fa5\u3000-\u303f\uff00-\uffefA-Za-z0-9_\-\.\,\:\;\/\\\(\)\[\]\{\}\%\+\*\#\@\!\?\~\`\'\"\s]{2,}/g;
  let m;
  while ((m = re.exec(s))) {
    const t = m[0].replace(/[\r\n\t]+/g, ' ').replace(/\s{2,}/g, ' ').trim();
    if (t.length >= 2 && !/^[0-9\s]+$/.test(t)) out.push(t);
  }
  return out;
}

function decodeWorkbook(b64) {
  const raw = Buffer.from(b64, 'base64');
  let out = null;
  for (const fn of [zlib.inflateSync, zlib.inflateRawSync, zlib.gunzipSync]) {
    try { out = fn(raw); break; } catch (e) { }
  }
  if (!out) throw new Error('inflate failed len=' + raw.length);
  return out;
}

function findWorkbook(obj) {
  // 递归收集所有长 base64 载荷（workbook 是元数据，related_sheet / attributed_text 才是单元格数据）
  const seen = new Set();
  const stack = [obj];
  const out = [];
  while (stack.length) {
    const cur = stack.pop();
    if (!cur || typeof cur !== 'object' || seen.has(cur)) continue;
    seen.add(cur);
    for (const k of Object.keys(cur)) {
      const v = cur[k];
      if (typeof v === 'string' && v.length > 800 && /^[A-Za-z0-9+/=\r\n]+$/.test(v.slice(0, 200))) out.push({ key: k, b64: v });
      else if (v && typeof v === 'object') stack.push(v);
    }
  }
  out.sort((a, b) => b.b64.length - a.b64.length);
  return out;
}

if (process.argv.includes('--selftest')) {
  const p1 = padIdOf('LIVE_T1_opendoc.json');
  const p2 = padIdOf('LIVE_T2_opendoc.json');
  console.log('PADID1=' + p1 + ' PADID2=' + p2);
  const j = JSON.parse(fs.readFileSync(path.join(DIR, 'LIVE_T1_opendoc.json'), 'utf8'));
  const wbs = findWorkbook(j);
  if (!wbs.length) { console.log('SELFTEST FAIL no payload'); process.exit(1); }
  let bytes = 0; const lines = [];
  for (const w of wbs) {
    try { const b = decodeWorkbook(w.b64); bytes += b.length; lines.push(...harvest(b)); } catch (e) { console.log('  skip ' + w.key + ' ' + e.message); }
  }
  const joined = lines.join('\n');
  console.log('SELFTEST inflate=' + bytes.length + 'B lines=' + lines.length);
  for (const probe of ['静置7天', 'MODEL_X', 'MODEL_Y', 'NAME_X', '立项']) {
    console.log('  probe ' + probe + ' => ' + (joined.includes(probe) ? 'HIT' : 'MISS'));
  }
  fs.writeFileSync(path.join(DIR, 'SELFTEST_TEXT.txt'), lines.join('\n'), 'utf8');
  console.log('SELFTEST WROTE SELFTEST_TEXT.txt');
  process.exit(0);
}

// ---------- 真实拉取 ----------
if (!fs.existsSync(CJ)) { console.log('NO_COOKIE_JSON ' + CJ + ' —— 需要先跑 re-yynote 的 grab 导出'); process.exit(2); }
const raw = JSON.parse(fs.readFileSync(CJ, 'utf8'));
const arr = Array.isArray(raw) ? raw : (raw.cookies || []);
const cookieHeader = arr.map(c => c.name + '=' + c.value).join('; ');
console.log('COOKIE n=' + arr.length + ' headerLen=' + cookieHeader.length);

const p1 = padIdOf('LIVE_T1_opendoc.json');
const p2 = padIdOf('LIVE_T2_opendoc.json');
if (!p1 || !p2) { console.log('NO_PADID p1=' + p1 + ' p2=' + p2); process.exit(3); }
console.log('PADID1=' + p1 + '\nPADID2=' + p2);

// 从 opendoc 头信息里动态取页签列表（subId + name + hidden），避免写死 subId 失效
async function discover(padId) {
  const url = BASE + '/dop-api/opendoc?id=' + encodeURIComponent(padId) + '&normal=1&outformat=1&noEscape=1';
  const res = await fetch(url, {
    headers: {
      'cookie': cookieHeader,
      'referer': BASE + '/sheet/' + padId,
      'user-agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36',
    },
  });
  const text = await res.text();
  const j = JSON.parse(text);
  const cv = j.clientVars || {};
  const col = cv.collab_client_vars || {};
  const header = col.header || [];
  const first = header[0] && header[0].d ? header[0].d : [];
  const sheets = first.filter(x => x && x.type === 'grid' && x.id).map(x => [x.id, x.name || x.id, !!x.hidden]);
  return { sheets, isLogin: !!cv.isLogin, padId: cv.localPadId || padId };
}

async function getSheet(padId, subId, xsort) {
  // 两个候选端点：get/sheet（按页签）与 opendoc（带 subId）；命中 retcode=0 即用
  const urls = [
    BASE + '/dop-api/get/sheet?padId=' + encodeURIComponent(padId) +
    '&subId=' + encodeURIComponent(subId) + '&startrow=0&endrow=-1&xsrf=&outformat=1&normal=1&preview_token=',
    BASE + '/dop-api/opendoc?id=' + encodeURIComponent(padId) +
    '&subId=' + encodeURIComponent(subId) + '&outformat=1&normal=1&noEscape=1',
  ];
  let last = null;
  for (const url of urls) {
    const t0 = Date.now();
    try {
      const res = await fetch(url, {
        headers: {
          'cookie': cookieHeader,
          'referer': BASE + '/sheet/' + padId,
          'x-requested-with': 'XMLHttpRequest',
          'accept': 'application/json, text/plain, */*',
          'user-agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36',
        },
      });
      const text = await res.text();
      const m = /"retcode"\s*:\s*(-?\d+)/.exec(text);
      const retcode = m ? Number(m[1]) : null;
      last = { status: res.status, ms: Date.now() - t0, text, retcode, url: url.slice(0, 80) };
      if (retcode === 0) return last;
    } catch (e) {
      last = { status: 0, ms: Date.now() - t0, text: 'ERR ' + e.message, retcode: null, url: url.slice(0, 80) };
    }
  }
  return last;
}

const out = [];
const stats = [];
async function run(doc, padId) {
  for (const [subId, name, hidden] of doc) {
    try {
      const r = await getSheet(padId, subId);
      const m = /"retcode"\s*:\s*(-?\d+)/.exec(r.text);
      const retcode = m ? Number(m[1]) : null;
      let lines = [];
      let bytes = 0;
      if (retcode === 0) {
        const j = JSON.parse(r.text);
        const wbs = findWorkbook(j);
        const hasData = wbs.some(w => w.key !== 'workbook');
        for (const w of wbs) {
          if (hasData && w.key === 'workbook') continue; // workbook 只是页签元数据，跳过以减少噪声
          try { const b = decodeWorkbook(w.b64); bytes += b.length; lines.push(...harvest(b)); } catch (e) { }
        }
      }
      stats.push({ subId, name, status: r.status, retcode, ms: r.ms, bytes, lines: lines.length });
      console.log('  ' + subId.padEnd(8) + ' ' + String(name).padEnd(24) + ' status=' + r.status + ' retcode=' + retcode + ' ms=' + r.ms + ' bytes=' + bytes + ' lines=' + lines.length);
      out.push('===== ' + subId + ' | ' + name + (hidden ? ' (hidden)' : '') + ' =====');
      lines.forEach((t, i) => out.push(String(i + 1).padStart(4, '0') + '  ' + t));
      out.push('');
    } catch (e) {
      console.log('  ' + subId + ' ERR ' + e.message);
      stats.push({ subId, name, error: e.message });
    }
  }
}

const t0 = Date.now();
// 动态发现页签：优先走 opendoc 头信息，失败才回退写死清单
let list1 = T1, list2 = T2;
try { const d = await discover(p1); if (d.sheets.length >= 5) { list1 = d.sheets; console.log('DISCOVER T1 sheets=' + d.sheets.length + ' isLogin=' + d.isLogin); } } catch (e) { console.log('DISCOVER T1 FAIL ' + e.message); }
try { const d = await discover(p2); if (d.sheets.length >= 3) { list2 = d.sheets; console.log('DISCOVER T2 sheets=' + d.sheets.length + ' isLogin=' + d.isLogin); } } catch (e) { console.log('DISCOVER T2 FAIL ' + e.message); }
console.log('--- T1 ---');
await run(list1, p1);
console.log('--- T2 ---');
await run(list2, p2);
fs.writeFileSync(OUT, out.join('\n'), 'utf8');
fs.writeFileSync(OUT_COPY, out.join('\n'), 'utf8');
fs.writeFileSync(path.join(DIR, 'ALL_HTTP_NODE_STATS.json'), JSON.stringify({ at: new Date().toISOString(), totalMs: Date.now() - t0, stats }, null, 2), 'utf8');
const ok = stats.filter(s => s.retcode === 0).length;
console.log('==== DONE ' + (Date.now() - t0) + 'ms | ok=' + ok + '/' + stats.length + ' | out=' + OUT + ' lines=' + out.length);
// 成功率不足 80% 或行数太少 → 非零退出，让管道判定抓取失败
process.exit(ok >= Math.ceil(stats.length * 0.8) && out.length > 2000 ? 0 : 1);
