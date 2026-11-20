// 页签变动检测：对比本轮抓到的页签清单与上一轮快照
// 用法: node sheet_diff.js [--reset]
// 输入: research/wxdata/wxwork_pull_stats.json (含 sheetsList)
//       research/sheets_snapshot.json  (上一轮基线，自动维护)
// 输出: research/sheets_diff.json + 控制台一行
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const STATS = path.join(ROOT, 'research', 'wxdata', 'wxwork_pull_stats.json');
const SNAP = path.join(ROOT, 'research', 'sheets_snapshot.json');
const DIFF = path.join(ROOT, 'research', 'sheets_diff.json');

function readJson(p) { try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch (e) { return null; } }

const stats = readJson(STATS);
if (!stats) { console.log('DIFF skip: no stats'); process.exit(0); }
const cur = Array.isArray(stats.sheetsList) ? stats.sheetsList : null;
if (!cur || !cur.length) { console.log('DIFF skip: stats has no sheetsList'); process.exit(0); }

const key = s => s.doc + ':' + s.id;
const curMap = new Map(cur.map(s => [key(s), s]));

if (process.argv.includes('--reset')) {
  fs.writeFileSync(SNAP, JSON.stringify({ at: new Date().toISOString(), sheets: cur }, null, 2), 'utf8');
  console.log('DIFF reset baseline sheets=' + cur.length);
  process.exit(0);
}

const snap = readJson(SNAP);
const old = snap && Array.isArray(snap.sheets) ? snap.sheets : [];
const oldMap = new Map(old.map(s => [key(s), s]));

const added = cur.filter(s => !oldMap.has(key(s)));
const removed = old.filter(s => !curMap.has(key(s)));
const renamed = cur.filter(s => oldMap.has(key(s)) && oldMap.get(key(s)).name !== s.name)
  .map(s => ({ doc: s.doc, id: s.id, from: oldMap.get(key(s)).name, to: s.name }));

const out = {
  at: new Date().toISOString(),
  baselineAt: snap ? snap.at : null,
  firstRun: !snap,
  count: cur.length,
  prevCount: old.length,
  added: added.map(s => ({ doc: s.doc, id: s.id, name: s.name })),
  removed: removed.map(s => ({ doc: s.doc, id: s.id, name: s.name })),
  renamed
};
fs.writeFileSync(DIFF, JSON.stringify(out, null, 2), 'utf8');
// 本轮清单成为下一轮基线（避免同一变动天天报）
fs.writeFileSync(SNAP, JSON.stringify({ at: out.at, sheets: cur }, null, 2), 'utf8');

if (out.firstRun) {
  console.log('DIFF baseline created sheets=' + out.count);
} else {
  console.log('DIFF sheets=' + out.count + ' (prev ' + out.prevCount + ') ' +
    'added=' + out.added.length + ' removed=' + out.removed.length + ' renamed=' + out.renamed.length);
  for (const a of out.added) console.log('  + ' + a.doc + ' ' + a.name + ' [' + a.id + ']');
  for (const r of out.removed) console.log('  - ' + r.doc + ' ' + r.name + ' [' + r.id + ']');
  for (const r of out.renamed) console.log('  ~ ' + r.doc + ' ' + r.from + ' -> ' + r.to);
}
