/* 里程碑视图逻辑：唯一实现。
   build_daily_brief.mjs 生成页面时把本文件内联进抽屉渲染处；测试用 node 直接 require。
   规则：日期归一（ISO / M/D / Excel 串号 46344(11/18)）→ 状态判定 → 按计划日期升序。
   圆点只认日期真相，文档状态原文另列，二者冲突时给出 conflict 文案。 */
var FRDMIL = (function () {
  function todayStr(now) {
    var d = now === undefined || now === null ? new Date() : new Date(now);
    return new Date(d.getTime() + 8 * 3600 * 1000).toISOString().slice(0, 10);
  }

  function isNum(s) {
    if (!s) return false;
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 48 || c > 57) return false;
    }
    return true;
  }

  function pad(n) {
    return ('0' + n).slice(-2);
  }

  function serial(k) {
    /* Excel 序列号合法区间 1..2958465（1899-12-31 .. 9999-12-31）；0 与超界一律判空，
       否则 '0000' 会算出 1899-12-30、'20261118' 会溢出成 +057373-01 */
    if (!(k >= 1 && k <= 2958465)) return '';
    var dt = new Date(Date.UTC(1899, 11, 30) + k * 86400000);
    return isNaN(dt.getTime()) ? '' : dt.toISOString().slice(0, 10);
  }

  function md(mo, dy, today) {
    if (!(mo >= 1 && mo <= 12 && dy >= 1 && dy <= 31)) return '';
    var y = Number(today.slice(0, 4));
    /* 取「与今天最近的一年」：先按今天所在年算，跨年超 183 天再挪一年 */
    var cand = y + '-' + pad(mo) + '-' + pad(dy);
    var diff = (Date.parse(cand) - Date.parse(today)) / 86400000;
    if (diff > 183) { y -= 1; cand = y + '-' + pad(mo) + '-' + pad(dy); }
    else if (diff < -183) { y += 1; cand = y + '-' + pad(mo) + '-' + pad(dy); }
    /* V8 会把 2026-02-30 滚成 03-02，必须反查月/日，否则不存在的日期会原样输出 */
    var d0 = new Date(Date.parse(cand));
    if (isNaN(d0.getTime()) || d0.getUTCMonth() + 1 !== mo || d0.getUTCDate() !== dy) return '';
    return cand;
  }

  /* ISO / 「11/18」 / Excel 串号「46344」「46344(11/18)」/ 「/」「-」空值 都归一成 YYYY-MM-DD */
  function isoOf(v, today) {
    var t = String(v === null || v === undefined ? '' : v).trim();
    if (!t) return '';
    if (t === '/' || t === '-') return '';
    if (t.charAt(4) === '-' && t.charAt(7) === '-') return t.slice(0, 10);
    var n = 0;
    while (n < t.length && isNum(t.charAt(n))) n++;
    if (n >= 4 && isNum(t.slice(0, n))) return serial(Number(t.slice(0, n)));
    var bar = t.indexOf('/');
    if (bar > 0 && bar === t.lastIndexOf('/')) {
      var a = t.slice(0, bar);
      var b = t.slice(bar + 1);
      if (isNum(a) && isNum(b) && a.length <= 2 && b.length <= 2) return md(Number(a), Number(b), today);
    }
    return '';
  }

  function mmdd(v) {
    return v ? (Number(v.slice(5, 7)) + '/' + Number(v.slice(8, 10))) : '';
  }

  /* done 已写回且不晚于计划 / late 延期完成 / over 计划已过未写回 / today 今天到期 / pend 未到期 / na 无计划 */
  function stateOf(dp, da, today) {
    if (da) return (dp && da > dp) ? 'late' : 'done';
    if (!dp) return 'na';
    if (dp < today) return 'over';
    if (dp === today) return 'today';
    return 'pend';
  }

  function list(ms, now) {
    var today = todayStr(now);
    var rows = (ms || []).map(function (m, i) {
      var dp = isoOf(m.p, today);
      var da = isoOf(m.a, today);
      var st = stateOf(dp, da, today);
      var word = String(m.s || '');
      var conflict = (word.indexOf('完成') >= 0 && !da) ? ('文档写「' + word + '」但没有实际完成日期') : '';
      return { n: String(m.n || ''), s: word, note: String(m.d || ''), dp: dp, da: da, st: st, conflict: conflict, i: i };
    }).sort(function (x, y) {
      var kx = x.dp || '9999-99-99';
      var ky = y.dp || '9999-99-99';
      if (kx !== ky) return kx < ky ? -1 : 1;
      return x.i - y.i;
    });
    return { today: today, rows: rows, mmdd: mmdd };
  }

  return { todayStr: todayStr, isoOf: isoOf, stateOf: stateOf, list: list, mmdd: mmdd, serial: serial };
})();

if (typeof module !== 'undefined' && module.exports) module.exports = FRDMIL;
