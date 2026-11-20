/* 里程碑逻辑单测：node research/lib/milestone_view.test.mjs
   只测纯函数，不碰页面。日期基线固定为 2026-11-19（Asia/Shanghai 口径）。 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const M = require('./milestone_view.js');

const TODAY = Date.parse('2026-11-19T10:00:00+08:00');
let fail = 0;
function eq(got, want, label) {
  const ok = JSON.stringify(got) === JSON.stringify(want);
  if (!ok) { fail++; console.log('FAIL ' + label + ' got=' + JSON.stringify(got) + ' want=' + JSON.stringify(want)); }
  return ok;
}

/* 1. 日期归一：四种来源都要落到同一个 ISO */
eq(M.isoOf('2026-11-18', '2026-11-19'), '2026-11-18', 'iso 直传');
eq(M.isoOf('46344(11/18)', '2026-11-19'), '2026-11-18', 'iso 串号带括号');
eq(M.isoOf('46344', '2026-11-19'), '2026-11-18', 'iso 纯串号');
eq(M.isoOf('11/18', '2026-11-19'), '2026-11-18', 'iso M/D');
eq(M.isoOf('', '2026-11-19'), '', 'iso 空');
eq(M.isoOf(null, '2026-11-19'), '', 'iso null');
// 假日期边界（reviewer 实测会渲染出 1899-12-30 / +057373-01 / 2027-02-30）
eq(M.isoOf('0000', '2026-11-19'), '', 'iso 串号 0 判空');
eq(M.isoOf('20261118', '2026-11-19'), '', 'iso 8 位文本日期判空');
eq(M.isoOf('2/30', '2026-11-19'), '', 'iso 不存在的日期判空');
eq(M.isoOf('2958466', '2026-11-19'), '', 'iso 串号超界判空');
eq(M.isoOf('/', '2026-11-19'), '', 'iso 斜杠');
eq(M.isoOf('未预立项', '2026-11-19'), '', 'iso 文本');

/* 2. 跨年：12/30 与 1/2 都要落到离今天最近的年份 */
eq(M.isoOf('12/30', '2026-11-19'), '2026-12-30', 'iso 年末');
eq(M.isoOf('1/2', '2026-11-19'), '2027-01-02', 'iso 跨年');

/* 3. 状态判定六档 */
eq(M.stateOf('2026-11-01', '2026-11-01', '2026-11-19'), 'done', 'state done');
eq(M.stateOf('2026-11-01', '2026-11-05', '2026-11-19'), 'late', 'state late');
eq(M.stateOf('2026-11-01', '', '2026-11-19'), 'over', 'state over');
eq(M.stateOf('2026-11-19', '', '2026-11-19'), 'today', 'state today');
eq(M.stateOf('2026-12-01', '', '2026-11-19'), 'pend', 'state pend');
eq(M.stateOf('', '', '2026-11-19'), 'na', 'state na');

/* 4. 排序：按计划日期升序，无计划排末，同日期保持文档顺序 */
const res = M.list([
  { n: 'C', p: '12/02', a: '', s: '未开始' },
  { n: 'A', p: '46303(10/8)', a: '46303(10/8)', s: '已完成' },
  { n: 'D', p: '', a: '', s: '月' },
  { n: 'B', p: '11/19', a: '', s: '未开始' },
  { n: 'E', p: '11/19', a: '', s: '未开始' }
], TODAY);
eq(res.rows.map((r) => r.n), ['A', 'B', 'E', 'C', 'D'], 'list 排序');
eq(res.rows[0].st, 'done', 'list A done');
eq(res.rows[1].st, 'today', 'list B today');
eq(res.rows[3].st, 'pend', 'list C pend');
eq(res.rows[4].st, 'na', 'list D na');

/* 5. 文档写「完成」但没有实际完成日期 → conflict 文案 */
const c = M.list([{ n: 'X', p: '11/02', a: '', s: '已完成' }], TODAY);
eq(c.rows[0].st, 'over', 'conflict 状态仍按日期判');
eq(c.rows[0].conflict.length > 0, true, 'conflict 有文案');

console.log(fail === 0 ? 'MILESTONE TEST OK' : ('MILESTONE TEST FAIL ' + fail));
process.exit(fail === 0 ? 0 : 1);
