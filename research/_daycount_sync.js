/* 逐日计数同步：以 window.__DAYD（明细面板数据源）为唯一真值，
   重算热力柱数字/柱高/分级、面板副标、KPI「未来节点」，杜绝两套口径。 */
(function () {
  var D = window.__DAYD || {};
  var days = Object.keys(D).sort();
  function cnt(d) {
    return (D[d] || []).length;
  }
  var counts = {};
  var total = 0;
  var models = {};
  days.forEach(function (d) {
    counts[d] = cnt(d);
    total += counts[d];
    (D[d] || []).forEach(function (x) { models[x.model] = 1; });
  });

  var hcols = [].slice.call(document.querySelectorAll('.hcol'));
  var max = 1;
  hcols.forEach(function (el) {
    var d = el.getAttribute('data-day');
    max = Math.max(max, counts[d] || 0);
  });
  hcols.forEach(function (el) {
    var d = el.getAttribute('data-day');
    var n = counts[d] || 0;
    var bar = el.querySelector('.hbar');
    var b = el.querySelector('.hbar b');
    if (bar) bar.style.height = (n ? Math.round(n / max * 100) : 0) + '%';
    if (b) b.textContent = n ? String(n) : '';
    el.className = 'hcol' + (el.classList.contains('now') ? ' now' : '') + (el.classList.contains('past') ? ' past' : '')
      + (n === 0 ? ' lv0 zero' : n === 1 ? ' lv1' : n === 2 ? ' lv2' : ' lv3');
    var tip = d.slice(5) + ' · ' + (D[d] || []).length + ' 条 · ' + n + ' 个节点 · ' + (new Set((D[d] || []).map(function (x) { return x.model; })).size) + ' 台机型';
    el.setAttribute('title', tip);
    el.setAttribute('aria-label', tip);
  });

  var totalModels = Object.keys(models).length;
  [].slice.call(document.querySelectorAll('.pt')).forEach(function (pt) {
    var t = (pt.querySelector('b') || {}).textContent || '';
    if (t.indexOf('未来 14 天节点密度') >= 0) {
      var sp = pt.querySelector('span');
      if (sp) sp.textContent = total + ' 条 · 覆盖 ' + totalModels + ' 台机型 · 点柱看当天';
    }
  });

  var kp = document.querySelector('[data-kpi="nodes"]');
  if (kp) {
    var v = kp.querySelector('.kpi-v');
    if (v) { v.innerHTML = total + '<em>个</em>'; v.setAttribute('data-n', String(total)); }
    var s2 = kp.querySelector('.kpi-s');
    if (s2) s2.textContent = '窗口共 ' + total;
    var bar2 = kp.querySelector('.kpi-b i');
    if (bar2) bar2.style.width = '100%';
  }
})();
