// 机型对齐检测：表格里出现的"在跑机型" vs 看板/台账收录的机型
// 用法: node project_diff.js
// 输入: research/wxdata/ALL_HTTP_TEXT.txt   （cookie 直连全页签文本）
//       research/brief_focus.json           （策展台账）
//       research/widget_data.json           （看板数据）
// 输出: research/projects_diff.json + 控制台摘要
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const TXT = path.join(ROOT, 'research', 'wxdata', 'ALL_HTTP_TEXT.txt');
const FOCUS = path.join(ROOT, 'research', 'brief_focus.json');
const WIDGET = path.join(ROOT, 'research', 'widget_data.json');
const OUT = path.join(ROOT, 'research', 'projects_diff.json');

// 保守：只认「FB/FD/FP/FZ 前缀 + 4~5 位数字(+可选 1 个字母)」，两侧不能紧贴 -/_（排除 GUID/色值/流水号）
const RE = /(?<![A-Za-z0-9\-_])((?:FB|FD|FP|FZ)\d{4,5}[A-Z]?)(?![A-Za-z0-9\-_])/g;
// 只统计「在跑的表」：进度表 / 预立项汇总 / 衍生品；排除报表注释、模板、延期分析、项目成员
const ACTIVE = /进度表|进度汇总表|衍生品/;
const SKIP = /报表注释|汇总表-模版|延期分析表|项目成员|停止/;

function readJson(p) { try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch (e) { return null; } }
function toks(s) { const out = []; let m; RE.lastIndex = 0; while ((m = RE.exec(String(s || '')))) out.push(m[1].toUpperCase()); return out; }

// 1) 表格里的机型
const lines = fs.existsSync(TXT) ? fs.readFileSync(TXT, 'utf8').split(/\r?\n/) : [];
const fromSheet = new Map();
let seg = '';
for (const ln of lines) {
  const h = ln.match(/^=====\s+(\S+)\s+\|\s+(.+?)\s+=====$/);
  if (h) { seg = h[2]; continue; }
  if (!ACTIVE.test(seg) || SKIP.test(seg)) continue;
  for (const t of toks(ln)) fromSheet.set(t, (fromSheet.get(t) || 0) + 1);
}

// 2) 看板/台账里的机型（台账 + 看板「在跑区」的项目卡）
//    不能读整块 HTML：V4 看板把归档机型也内嵌在 __DRAW 里（archivedAt / running:false），
//    按全文提型号会让 boardModels 19→38、missingInSheet 6→24，指标随看板改版持续误报。
const BOARD_HTML = path.join(ROOT, '项目数据看板.html');
const board = new Map();
const add = (s, src) => { for (const t of toks(s)) { if (!board.has(t)) board.set(t, src); } };
const focus = readJson(FOCUS);
for (const f of (focus && focus.focus) || []) add(f.model, '台账');
let boardCards = 0;
if (fs.existsSync(BOARD_HTML)) {
  const html = fs.readFileSync(BOARD_HTML, 'utf8');
  const pcRe = /class="pc"[^>]*data-model="([^"]+)"/g;
  let mm;
  while ((mm = pcRe.exec(html))) { add(mm[1], '看板'); boardCards++; }
  if (!boardCards) add(html, '看板(全文回退)');  // 卡片结构再改时不至于静默变空
}

// 3) 差集（按「数字段」归族：ITEM_X / ITEM_X / ITEM_X 视为同一项目族）
const fam = t => (t.match(/\d{4,6}/) || [t])[0].replace(/^0+/, '');
const boardFams = new Set([...board.keys()].map(fam));
const missing = [];
for (const [t, n] of fromSheet) {
  if (board.has(t) || boardFams.has(fam(t))) continue;
  missing.push({ model: t, hits: n });
}
missing.sort((a, b) => b.hits - a.hits || a.model.localeCompare(b.model));

const sheetFams = new Set([...fromSheet.keys()].map(fam));
const notInSheet = [];
for (const [t, src] of board) if (!sheetFams.has(fam(t))) notInSheet.push({ model: t, src });

const out = {
  at: new Date().toISOString(),
  sheetModels: fromSheet.size,
  boardModels: board.size,
  boardCards,
  missingOnBoard: missing,
  missingInSheet: notInSheet
};

// 4) 与上期快照比：本期新出现的「未收录机型」= 最可能是新增项目
const SNAP = path.join(ROOT, 'research', 'projects_snapshot.json');
const prev = readJson(SNAP);
const prevSet = new Set(((prev && prev.missingOnBoard) || []).map(x => x.model));
const newlyAppeared = prev ? missing.filter(m => !prevSet.has(m.model)).map(m => m.model) : [];
out.baselineAt = prev ? prev.at : null;
out.newlyAppeared = newlyAppeared;
fs.writeFileSync(SNAP, JSON.stringify({ at: out.at, missingOnBoard: missing }, null, 2), 'utf8');

fs.writeFileSync(OUT, JSON.stringify(out, null, 2), 'utf8');

console.log('PROJDIFF sheet=' + out.sheetModels + ' board=' + out.boardModels +
  ' 表格有/看板无=' + missing.length + ' 看板有/表格无=' + notInSheet.length +
  ' 本期新增未收录=' + newlyAppeared.length);
for (const m of missing.slice(0, 15)) console.log('  + ' + m.model + ' ×' + m.hits);
if (newlyAppeared.length) console.log('  NEW: ' + newlyAppeared.join(', '));
for (const m of notInSheet.slice(0, 6)) console.log('  - ' + m.model + ' (' + m.src + ')');
