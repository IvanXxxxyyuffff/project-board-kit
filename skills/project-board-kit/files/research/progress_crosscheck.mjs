// 群聊 × 企微文档节点 的进度口径交叉校验
// 输入：项目数据看板.html 的 window.__DRAW（每项目 ms:[{n,p,a,late}]）、raw/emobile/project_chatlog.json
// 输出：research/progress_conflicts.json
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

let ROOT = path.dirname(fileURLToPath(import.meta.url)).replace(/\\/g, '/');
while (ROOT && !fs.existsSync(ROOT + '/每日项目简报.ps1')) {
  const parent = path.dirname(ROOT).replace(/\\/g, '/');
  if (!parent || parent === ROOT) break;
  ROOT = parent;
}
const R = ROOT + '/research';

function extractDraw(html) {
  const i = html.indexOf('window.__DRAW=');
  if (i < 0) throw new Error('no __DRAW');
  let s = i + 'window.__DRAW='.length;
  // 跳到第一个 {
  while (s < html.length && html[s] !== '{') s++;
  let depth = 0, inStr = false, esc = false;
  for (let k = s; k < html.length; k++) {
    const ch = html[k];
    if (inStr) {
      if (esc) esc = false;
      else if (ch === '\\') esc = true;
      else if (ch === '"') inStr = false;
      continue;
    }
    if (ch === '"') { inStr = true; continue; }
    if (ch === '{') depth++;
    else if (ch === '}') {
      depth--;
      if (depth === 0) return JSON.parse(html.slice(s, k + 1));
    }
  }
  throw new Error('unbalanced __DRAW');
}

const DRAW = extractDraw(fs.readFileSync(ROOT + '/项目数据看板.html', 'utf8'));
const chatRaw = JSON.parse(fs.readFileSync(R + '/raw/emobile/project_chatlog.json', 'utf8'));
const groups = Array.isArray(chatRaw) ? chatRaw : (chatRaw.groups || []);

// 本机 E-MOBILE 是否有该机型的会话/群：没有群的机型不是本项目在跟的东西（多为别的客户的单被顺带提到），不进诊断
const convNames = (() => {
  try {
    const cj = JSON.parse(fs.readFileSync(R + '/raw/emobile/conversations_latest.json', 'utf8'));
    const arr = cj.conversations || cj.list || cj.data || [];
    const names = [...new Set(arr.flatMap(c => [c.groupName, c.targetname, c.content]).filter(Boolean).map(s => String(s).toUpperCase()))];
    return names.length ? names : null;
  } catch (e) { return null; }
})();
const hasOwnGroup = toks => !convNames || toks.some(t => convNames.some(n => n.includes(t)));

const msgs = [];
for (const g of groups) {
  for (const m of (g.messages || [])) {
    msgs.push({ t: String(m.time || ''), s: String(m.sender || ''), x: String(m.text || ''), g: String(g.group || '') });
  }
}

const NOW = new Date();
const DAY = 86400000;
function ageDays(t) {
  const d = new Date(String(t).replace(' ', 'T'));
  if (isNaN(d)) return 999;
  return (NOW - d) / DAY;
}
function planDate(p) {
  const s = String(p || '').trim();
  if (!s) return null;
  if (/^\d{4}-\d{1,2}-\d{1,2}$/.test(s)) return new Date(s + 'T00:00:00');
  if (/^\d{1,2}\/\d{1,2}$/.test(s)) {
    const [m, d] = s.split('/').map(Number);
    let dt = new Date(NOW.getFullYear(), m - 1, d);
    // 跨年：M/D 落在半年前以前，视为下一年计划
    if (dt.getTime() < NOW.getTime() - 180 * DAY) dt = new Date(NOW.getFullYear() + 1, m - 1, d);
    return dt;
  }
  if (/^\d{5}$/.test(s)) return new Date(Date.UTC(1899, 11, 30) + Number(s) * DAY);
  return null;
}

const DONE = /(已完成|已送样|送样了|已试制|试制完成|批量生产了|已批量生产|已开模|开模完成|已交付|出货了|已关闭|已结案|归档)/;
const LATE = /(延期|推迟|改期|来不及|卡在|延误|拖了|滞后|滑期|往后)/;
const BLOCK = /(待|等|催|未回|没人回|未确认|缺|异常|功能异常|结构异常|质量异常|暂停)/;

const rows = [];
const skipped = [];
for (const [model, p] of Object.entries(DRAW)) {
  const up = String(model).toUpperCase();
  const toks = [...new Set([
    ...(up.match(/(?:FB|FD|FP|FZ)\d{4,5}[A-Z]?/g) || []),
    ...(up.match(/(?<!\d)\d{4,5}(?!\d)/g) || []),
    ...(up.match(/ITEM_X[A-Z]?/g) || []),
    up,
  ])];
  if (!hasOwnGroup(toks)) { skipped.push({ model, reason: '本机 E-MOBILE 无该机型群' }); continue; }
  const hit = msgs.filter(m => ageDays(m.t) <= 21 && toks.some(t => m.x.toUpperCase().includes(t) || m.g.toUpperCase().includes(t)));
  const MS_KW = /(试制|送样|批量生产|开模|环测|归档|立项|样件|DVT|T0|T1|工装|试模|夹具)/i;
  const doneM = hit.filter(m => DONE.test(m.x) && MS_KW.test(m.x));
  const lateM = hit.filter(m => LATE.test(m.x));
  const blockM = hit.filter(m => BLOCK.test(m.x));
  const ms = (p.ms || []).map(x => ({ n: x.n, plan: x.p, actual: x.a }));
  const filledRatio = ms.length ? ms.filter(m => String(m.actual || '').trim()).length / ms.length : 0;
  const overdue = ms.filter(m => {
    const d = planDate(m.plan);
    return d && d < NOW && !String(m.actual || '').trim();
  });
  const conf = [];
  // 1) 群聊说完成/试制，节点表几乎没实际进度
  if (doneM.length >= 1 && ms.length && filledRatio < 0.25) {
    conf.push(`群聊出现 ${doneM.length} 条完成/试制口径，但节点表实际完成仅 ${Math.round(filledRatio * 100)}%`);
  }
  // 2) 群聊说延期，计划日期没动（且无逾期节点）
  if (lateM.length >= 1 && overdue.length === 0) {
    conf.push(`群聊出现 ${lateM.length} 条延期/推迟口径，节点计划日期未见调整`);
  }
  // 3) 已逾期却没有群内进展
  if (overdue.length >= 1 && hit.length === 0) {
    conf.push(`已逾期节点 ${overdue.length} 个，近 21 天群里无任何进展消息`);
  }
  // 4) 群里在催没人回，但台账无卡点
  if (blockM.length >= 2 && !String(p.blocker || '').trim()) {
    conf.push('群里连续催办/报异常，台账卡点为空');
  }
  if (!conf.length) continue;

  const ev = [...doneM, ...lateM, ...blockM]
    .sort((a, b) => (a.t < b.t ? 1 : -1))
    .slice(0, 3)
    .map(m => `${String(m.t).slice(5, 16)} ${m.s.replace(/\(.*\)$/, '')}：${m.x.replace(/\s+/g, ' ').slice(0, 60)}`);

  rows.push({
    model,
    priority: p.prio || '',
    owner: p.owner || '',
    status: p.status || '',
    nodeFilled: `${ms.filter(m => String(m.actual || '').trim()).length}/${ms.length}`,
    overdue: overdue.map(m => `${m.n}(${m.plan})`).slice(0, 4),
    conflicts: conf,
    chatMsgs21d: hit.length,
    evidence: ev,
  });
}

rows.sort((a, b) => {
  const w = s => (s === 'P0' ? 0 : s === 'P1' ? 1 : 2);
  return w(a.priority) - w(b.priority) || a.model.localeCompare(b.model);
});

fs.writeFileSync(R + '/progress_conflicts.json', JSON.stringify({ at: new Date().toISOString(), projects: Object.keys(DRAW).length, scanned: Object.keys(DRAW).length - skipped.length, skipped, rows }, null, 1), 'utf8');

console.log(`CROSSCHECK projects=${Object.keys(DRAW).length} msgs=${msgs.length} conflicts=${rows.length} skipped=${skipped.length}`);
for (const s of skipped) console.log(`  - 跳过 ${s.model}（${s.reason}）`);
for (const r of rows) {
  console.log(`\n[${r.priority}] ${r.model} 节点 ${r.nodeFilled} 近21天 ${r.chatMsgs21d} 条`);
  for (const c of r.conflicts) console.log('  ! ' + c);
  if (r.overdue.length) console.log('  逾期: ' + r.overdue.join(', '));
  for (const e of r.evidence) console.log('   · ' + e);
}
