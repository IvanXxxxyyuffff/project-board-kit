# -*- coding: utf-8 -*-
"""把真实表格数据注入 V4 看板（幂等）。

职责：把 ``sheet_canon.json``（86 机型 / 品牌 / 归档 / 节点表）注入 V4 看板，
生成「品牌分布」双列面板、「已归档」浮层、品牌筛选芯片，并用真实节点表
同步项目卡进度与抽屉数据岛 ``window.__DRAW``。被每日项目简报.ps1 的 5a 调用。

设计要点（2026-11-14 审核修复，勿回退）：
- 基线优先取现役 OUT（项目数据看板.html），TMPL 仅作首次兜底；这样每轮 5a
  重建不会把用户在看板页上的手工版面还原回模板（用户多次投诉「UI 被还原」）。
- ``strip_injected()`` 按精确 id 边界逐块删除上一轮注入内容，幂等。旧版用
  ``<style ...>.*?</script>`` 单条正则跨删了模板自带的 ``@media(max-width:900px)``
  与 ``#st-wrap`` 的 grid 双列，导致响应式布局丢失——这是 blocker#1 的事故源。
- 合并后的搜索逻辑只有一份（旧版 sheet-truth 与 st-search-fix 双绑定同一
  ``#q`` / ``#st-qhits``，setTimeout 600 与 800 竞态，且清空搜索不恢复卡片）。
- 写盘用 ``os.replace`` 原子替换，避免半写文件被挂件读到。
- 路径全部相对 ``__file__`` 上溯两级，分发到同事机后无需改盘符。
"""
import io, time, json, os, re, datetime

ROOT  = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TMPL  = os.path.join(ROOT, 'research', '_模板_v4.html')
OUT   = os.environ.get('V4_OUT') or os.path.join(ROOT, '项目数据看板.html')
CANON = os.path.join(ROOT, 'research', 'sheet_canon.json')

canon = json.load(io.open(CANON, encoding='utf-8'))
payload = json.dumps(canon, ensure_ascii=False, separators=(',', ':'))

# 现役成品优先；仅当缺失/异常小才回退到 V4 模板副本
_base = OUT if (os.path.exists(OUT) and os.path.getsize(OUT) > 120000) else TMPL
# 回滚：必须保留用户的 V4 版面（OUT/模板）；生成页只用于取数据，不能当基线。
def _stamp_dates(h):
    """只刷新 <title> 与页脚「数据源/生成」时间戳，不动任何版面结构。
    用户的 V4 静态区（热力柱/群聊/项目卡）保持原样，只保证看板不会长期显示旧日期。"""
    today = datetime.date.today().isoformat()
    h = re.sub(r'<title>[^<]*</title>', '<title>项目看板 · %s</title>' % today, h, count=1)
    def _f(m):
        return '数据源 %s · 生成 %s' % (m.group(1), datetime.datetime.now().strftime('%m-%d %H:%M'))
    h = re.sub(r'数据源 (\d{2}-\d{2} \d{2}:\d{2}) · 生成 \d{2}-\d{2} \d{2}:\d{2}', _f, h, count=1)
    return h
html = io.open(_base, encoding='utf-8').read()

script = r'''
<style id="sheet-truth-css">
#st-wrap{max-width:1560px;margin:0 auto 28px;padding:0 6px;display:grid;grid-template-columns:1.15fr 1fr;gap:14px}
@media(max-width:900px){#st-wrap{grid-template-columns:1fr}}
#st-wrap .stc{background:var(--surf,#fff);border:1px solid var(--line,#e6e8ee);border-radius:18px;padding:16px 18px;min-width:0}
#st-wrap h3{margin:0 0 4px;font-size:14px;letter-spacing:.02em}
#st-wrap .sub{font-size:11.5px;color:var(--ink3,#8891a4);margin-bottom:12px}
#st-bars{display:flex;flex-direction:column;gap:9px}
#st-bars .row{display:grid;grid-template-columns:104px 1fr 40px;align-items:center;gap:10px;font-size:12px}
#st-bars .tr{display:block;height:9px;border-radius:6px;background:rgba(120,130,150,.16);overflow:hidden}
#st-bars .row>span:first-child{color:var(--ink,#e9ecf2)}
#st-bars .fl{display:block;height:100%;min-width:3px;border-radius:6px;transition:width .8s cubic-bezier(.2,.8,.2,1)}
#st-bars .n{text-align:right;font-variant-numeric:tabular-nums;color:var(--ink2,#5b6476)}
#st-arch-inline{margin-top:2px}
#st-arch-open{margin-top:10px;border:1px solid var(--line,#2a2f38);background:transparent;color:inherit;border-radius:9px;padding:4px 12px;cursor:pointer;font:inherit;font-size:12px}
#st-fs button{display:inline-flex;gap:6px;align-items:center}
#st-fs button b{font-weight:600;opacity:.75;font-variant-numeric:tabular-nums}
#st-modal{position:fixed;inset:0;z-index:99;background:rgba(8,10,14,.72);backdrop-filter:blur(6px);display:none}
#st-modal.on{display:block}
#st-modal .box{position:absolute;left:50%;top:8vh;transform:translateX(-50%);width:min(880px,92vw);max-height:78vh;overflow:auto;background:var(--surf,#14171d);border:1px solid var(--line,#2a2f38);border-radius:18px;padding:18px 20px}
#st-modal h3{margin:0 0 2px;font-size:15px}
#st-modal .sub{font-size:11.5px;color:var(--ink3,#8891a4);margin-bottom:12px}
#st-modal .x{position:absolute;right:16px;top:14px;border:1px solid var(--line,#2a2f38);background:transparent;color:inherit;border-radius:9px;padding:3px 10px;cursor:pointer;font:inherit;font-size:12px}
#st-list,#st-arch-inline{display:grid;grid-template-columns:1fr 1fr;gap:0 26px;font-size:12.5px}
#st-arch-inline{grid-template-columns:1fr}
@media(max-width:720px){#st-list{grid-template-columns:1fr}}
#st-list .a,#st-arch-inline .a{display:flex;justify-content:space-between;gap:10px;padding:6px 0;border-bottom:1px dashed rgba(120,130,150,.18)}
#st-list .a i,#st-arch-inline .a i{color:var(--ink3,#8891a4);font-style:normal;font-variant-numeric:tabular-nums}
</style>
<div id="st-wrap">
  <section class="stc">
    <h3>品牌分布</h3>
    <div class="sub">来源：企微表格「汇总表」真实品牌列 · 共 <b id="st-total">0</b> 个机型</div>
    <div id="st-bars"></div>
  </section>
  <section class="stc">
    <h3>已归档</h3>
    <div class="sub">状态 = 已结案，按归档日期倒序 · 共 <b id="st-acount-inline">0</b> 条</div>
    <div id="st-arch-inline"></div>
    <button type="button" id="st-arch-open">查看全部 →</button>
  </section>
</div>
<div id="st-modal"><div class="box"><button class="x" id="st-close">关闭</button>
  <h3>已归档</h3><div class="sub">状态 = 已结案，按归档日期倒序 · 共 <b id="st-acount">0</b> 条</div>
  <div id="st-list"></div></div></div>
<script id="sheet-truth">
(function(){
  var SHEET = __SHEET__;
  window.__SHEET = SHEET;
  var models = Object.keys(SHEET).map(function(k){return SHEET[k]});
  var COL = {'CLIENT_A':'#41BA52','其他客户':'#5B8DEF','SELF_CO':'#F5A623','OEM单':'#B27CF5','未标注':'#8891A4','Pikus':'#E4572E','019':'#3FB6C6'};
  var groups = {};
  models.forEach(function(m){
    var g = m.brandGroup || '未标注';
    groups[g] = (groups[g]||0) + 1;
  });
  var order = ['CLIENT_A','其他客户','SELF_CO','OEM单','未标注'];
  var keys = order.filter(function(k){return groups[k]});
  Object.keys(groups).forEach(function(k){ if(keys.indexOf(k)<0) keys.push(k) });
  var max = Math.max.apply(null, keys.map(function(k){return groups[k]}).concat([1]));
  document.getElementById('st-total').textContent = models.length;
  document.getElementById('st-bars').innerHTML = keys.map(function(k){
    var w = Math.round(groups[k]/max*100);
    return '<div class="row"><span>'+k+'</span><span class="tr"><i class="fl" style="width:'+w+'%;background:'+(COL[k]||'#8891A4')+'"></i></span><span class="n">'+groups[k]+'</span></div>';
  }).join('');

  var arch = models.filter(function(m){return m.status==='已结案'});
  var key = function(m){var v=(m.archivedAt||'').trim();if(!v)return 0;var p=v.split('-');if(p.length===3)return (+p[0])*10000+(+p[1])*100+(+p[2]);var q=v.split('/');return q.length===2?(+q[0])*100+(+q[1]):0};
  var byDate = arch.slice().sort(function(a,b){ return key(b)-key(a) });
  var rowHtml = function(m){ return '<div class="a"><span>'+m.model+' <i>· '+(m.brand||'—')+'</i></span><i>'+(m.archivedAt||'—')+'</i></div>'; };
  document.getElementById('st-acount').textContent = arch.length;
  document.getElementById('st-list').innerHTML = byDate.map(rowHtml).join('');

  // 第二列「已归档」摘要 + 浮层开关；浮层关闭三件套只绑一次
  var modal = document.getElementById('st-modal');
  var inline = document.getElementById('st-arch-inline');
  if (inline) inline.innerHTML = byDate.slice(0,6).map(rowHtml).join('');
  var ic = document.getElementById('st-acount-inline'); if (ic) ic.textContent = arch.length;
  var ob = document.getElementById('st-arch-open');
  if (ob && modal) ob.addEventListener('click', function(){ modal.classList.add('on'); });
  if (modal) {
    var xc = document.getElementById('st-close'); if (xc) xc.addEventListener('click', function(){ modal.classList.remove('on'); });
    modal.addEventListener('click', function(e){ if (e.target === modal) modal.classList.remove('on'); });
    document.addEventListener('keydown', function(e){ if (e.key === 'Escape') modal.classList.remove('on'); });
  }
  var fs = document.getElementById('fs');
  if (fs && !document.getElementById('st-arch-btn')) {
    var btn = document.createElement('button');
    btn.id = 'st-arch-btn'; btn.type = 'button';
    btn.innerHTML = '已归档 <b>' + arch.length + '</b>';
    fs.appendChild(btn);
    fs.addEventListener('click', function(e){
      if (e.target.closest('#st-arch-btn')) { e.preventDefault(); e.stopImmediatePropagation(); if (modal) modal.classList.add('on'); }
    }, true);
  }

  // 同名负责人分组合并（字体异体 / 别名导致同名两组）
  (function mergeGroups(){
    var wrap = document.getElementById('groups'); if(!wrap) return;
    var map = {};
    [].slice.call(wrap.querySelectorAll('.grp')).forEach(function(g){
      var b = g.querySelector('.grp-h b'); if(!b) return;
      var k = b.textContent.trim();
      if(!map[k]) { map[k] = g; return; }
      var dst = map[k].querySelector('.pcs'), src = g.querySelector('.pcs');
      if(dst && src) while(src.firstElementChild) dst.appendChild(src.firstElementChild);
      g.remove();
    });
    Object.keys(map).forEach(function(k){
      var g = map[k], pcs = g.querySelector('.pcs'), em = g.querySelector('.grp-h em');
      if(em && pcs) em.textContent = pcs.children.length + ' 个';
      var p0 = 0;
      if(pcs) [].forEach.call(pcs.children, function(c){ if(c.getAttribute('data-prio')==='P0') p0++; });
      var tag = g.querySelector('.grp-p0');
      if(tag) { if(p0) tag.textContent = 'P0 ' + p0; else tag.remove(); }
    });
  })();

  // 搜索：卡片同步过滤 + 「表格里其它匹配」（含已归档）。仅此一份，避免双绑定竞态；
  // 清空关键词时恢复全部卡片，否则会把上一轮过滤状态留在页面上。
  (function(){
    var q = document.getElementById('q'); if(!q) return;
    var g = document.getElementById('groups');
    function box(){
      var b = document.getElementById('st-qhits');
      if(!b){
        b = document.createElement('div'); b.id = 'st-qhits';
        b.style.cssText = 'display:none;margin:16px 0 0';
        if(g && g.parentNode) g.parentNode.insertBefore(b, g.nextSibling); else document.body.appendChild(b);
      }
      return b;
    }
    function run(){
      var t = (q.value||'').trim().toLowerCase(), b = box();
      if(!t){
        b.style.display = 'none'; b.innerHTML = '';
        [].forEach.call(document.querySelectorAll('.pc'), function(c){ c.style.display = ''; });
        return;
      }
      var on = {};
      [].forEach.call(document.querySelectorAll('.pc'), function(c){
        var m = c.getAttribute('data-model'); if(m) on[m] = 1;
        var v = SHEET[m] || {}, hay = (m+' '+(v.brand||'')+' '+(v.brandGroup||'')).toLowerCase();
        c.style.display = hay.indexOf(t) >= 0 ? '' : 'none';
      });
      var hit = models.filter(function(m){
        return ((m.model||'')+' '+(m.brand||'')+' '+(m.brandGroup||'')).toLowerCase().indexOf(t) >= 0;
      });
      var extra = hit.filter(function(m){ return !on[m.model]; });
      if(!extra.length){ b.style.display = 'none'; b.innerHTML = ''; return; }
      var rows = extra.slice(0,60).map(function(v){
        return '<div class="a"><span>'+(v.model||'')+' <i>· '+(v.brandGroup||'')+'</i></span><i>'
          +(v.status||'')+(v.archivedAt?(' '+v.archivedAt):'')+'</i></div>';
      }).join('');
      b.innerHTML = '<div class="grp-h" style="margin:0 0 8px"><b>表格里其它匹配</b><span>'
        + extra.length + ' 台 · 含已归档（不在上面的项目卡里）</span></div>'
        + '<div style="display:grid;grid-template-columns:1fr 1fr;gap:0 26px;font-size:12.5px">' + rows + '</div>';
      b.style.display = 'block';
    }
    q.addEventListener('input', run); q.addEventListener('keyup', run); q.addEventListener('search', run);
    setTimeout(run, 600);
  })();

  // 用真实节点表同步卡片进度 / 抽屉数据岛
  function syncCards(){
    var D = window.__DRAW || (window.__DRAW = {});
    var cards = [].slice.call(document.querySelectorAll('.pc'));
    cards.forEach(function(c){
      var model = c.getAttribute('data-model'); if(!model) return;
      var m = (function(mm){
        if(SHEET[mm] && (SHEET[mm].msTotal||0)>0) return SHEET[mm];
        var nz=function(x){return (x||'').replace(/[（）()\s]/g,'').replace(/开模|一次性|双色模/g,'')};
        var nm=nz(mm), best=null;
        Object.keys(SHEET).forEach(function(k){
          var nk=nz(k); if(!nk) return;
          if(nk===nm || nk.indexOf(nm)>=0 || nm.indexOf(nk)>=0){
            var v=SHEET[k]; if((v.msTotal||0) > (best?(best.msTotal||0):0)) best=v;
          }
        });
        return best || SHEET[mm];
      })(model);
      if(!m) return;
      var total = m.msTotal || 0, done = m.msDone || 0;
      var prog = c.querySelector('.pc-prog'), em = c.querySelector('.pc-nums em');
      if (total > 0) {
        var pct = Math.round(done/total*100);
        if (prog) { prog.className = 'pc-prog'; prog.setAttribute('title', '里程碑完成 '+done+'/'+total); prog.innerHTML = '<i style="width:'+pct+'%"></i>'; }
        if (em) em.textContent = done + '/' + total;
      } else {
        if (prog) { prog.className = 'pc-prog none'; prog.removeAttribute('title'); prog.innerHTML = '无节点表'; }
        if (em) em.textContent = m.status === '已结案' ? '已归档' : '—';
      }
      var d = D[model];
      if (total > 0) {
        if (!d) { d = D[model] = { model: model, prio: c.getAttribute('data-prio')||'P3', stKey: 'other' }; }
        d.ms = (m.milestones||[]).map(function(x){ return {n:x.n, p:x.p, a:x.a, s:x.s, d:x.d} });
        d.prog = { done: done, total: total };
        if (!d.brand && m.brand) d.brand = m.brand;
        if (!d.status && m.status) d.status = m.status;
      }
    });
  }
  document.addEventListener('DOMContentLoaded', syncCards);
  window.addEventListener('load', syncCards);
  setTimeout(syncCards, 200); setTimeout(syncCards, 900);

  // 品牌筛选：给项目卡打 data-bg，再按分组过滤
  var cards = [].slice.call(document.querySelectorAll('.pc'));
  cards.forEach(function(c){
    var b = c.querySelector('b[title]');
    var model = c.getAttribute('data-model') || (b && b.getAttribute('title')); if(!model) return;
    var mm = SHEET[model];
    if (mm) c.setAttribute('data-bg', (mm.brandGroup||'未标注'));
    else c.setAttribute('data-bg','未标注');
  });
  var oldC = document.getElementById('st-chips'); if (oldC) oldC.parentNode.removeChild(oldC);
  var chips = document.createElement('div');
  chips.id = 'st-chips'; chips.className = 'fs';
  chips.innerHTML = ['全部'].concat(keys).map(function(k,i){
    return '<button data-g="'+k+'" aria-pressed="'+(i===0)+'">'+k+'</button>';
  }).join('');
  chips.style.cssText = 'display:flex;gap:8px;flex-wrap:wrap;margin:4px 0 12px';
  var barsEl = document.getElementById('st-bars'); if (barsEl) barsEl.parentNode.insertBefore(chips, barsEl);
  chips.addEventListener('click', function(e){
    var t = e.target.closest('button'); if(!t) return;
    [].forEach.call(chips.children, function(b){ b.setAttribute('aria-pressed', String(b===t)); b.className = b===t ? 'on' : '' });
    var g = t.getAttribute('data-g');
    cards.forEach(function(c){
      var show = (g==='全部') || (c.getAttribute('data-bg')===g);
      c.style.display = show ? '' : 'none';
    });
  });
})();
</script>
'''.replace('__SHEET__', payload)


def _div_span(h, i):
    """从 ``<div ...`` 起点 i 找配平的 ``</div>``，返回结束下标（含）。

    st-wrap / st-modal 内含嵌套 div，正则 ``.*?</div>`` 会在第一个内层
    ``</div>`` 截断，删不干净；必须用深度计数。
    """
    d = 0; j = i
    while j < len(h):
        if h.startswith('<div', j):
            d += 1; j += 4; continue
        if h.startswith('</div>', j):
            d -= 1; j += 6
            if d == 0:
                return j
            continue
        j += 1
    return -1


def _eat_blank(h, j):
    """吃掉 j 之后的纯空白（含换行），返回新下标，保证多轮注入不累积空行。"""
    while j < len(h) and h[j] in ' \t\r\n':
        j += 1
    return j


def strip_injected(h):
    """幂等清理：按精确 id 边界删除上一轮注入的 style/script/div 块。

    历史事故：旧版用 ``<style id="sheet-truth-css">.*?</script>`` 单条正则，
    非贪婪跨过 ``</style>`` 一路吞到 sheet-truth 的 ``</script>``，连同模板
    自带的 media900 与 grid 双列一起删掉。故此处逐块精确删除：style/script
    闭合标签自明用正则（已带 ``\\s*``），div 用深度配平后额外吃掉尾部空白，
    否则每轮 5a 都会多留一个 ``\\n``，成品 MD5 永远漂移。
    """
    h = re.sub(r'<style id="sheet-truth-css">.*?</style>\s*', '', h, flags=re.S)
    # 只删本脚本自己注入的 script；st-search-fix 是旧版残留（搜索已合并，需清一次）。
    # ring-svg-fix/2 由 5a 的 _ring_fix3.py 负责（自带按 id 幂等），inject 不碰，
    # 否则「单跑 inject」会把圆环删掉、且以 OUT 为基线时丢失上轮圆环。
    for sid in ('sheet-truth', 'st-search-fix'):
        h = re.sub(r'<script id="' + sid + r'">.*?</script>\s*', '', h, flags=re.S)
    for did in ('st-wrap', 'st-modal', 'st-qhits', 'st-chips'):
        while True:
            i = h.find('<div id="' + did + '"')
            if i < 0:
                break
            j = _div_span(h, i)
            if j < 0:
                break
            j = _eat_blank(h, j)
            h = h[:i] + h[j:]
    # 注入锚点前的空白统一清零；否则 script 块自带的首/尾换行每轮累积 +1，
    # 成品 MD5 永远漂移（曾实测三轮 285626→285628→285630）。
    h = re.sub(r'\s*</body>', '</body>', h, count=1)
    return h


def move_panel_html(h):
    """构建期把 #st-wrap 挪到 #groups 所属 section 之前，返回新 HTML（纯函数）。

    面板位置在构建期一次定死，运行期 JS 不再搬动（历史教训：运行期反复
    insertBefore 导致面板漂移、位置断言假绿）。
    """
    i = h.find('<div id="st-wrap"')
    if i < 0:
        return h
    j = _div_span(h, i)
    if j < 0:
        return h
    block = h[i:j]
    rest = h[:i] + h[j:]
    gid = rest.find('id="groups"')
    if gid < 0:
        return h
    p0 = rest.rfind('<section', 0, gid)
    if p0 < 0:
        p0 = rest.rfind('<div', 0, gid)
    if p0 < 0:
        return h
    return rest[:p0] + block + '\n' + rest[p0:]


# 1) 删旧注入块（无论基线是 OUT 还是 TMPL，都收敛到同一「裸版面」）
html = strip_injected(html)
# 2) 注入唯一一份合并块（style + 双列 st-wrap + 浮层 + sheet-truth script）
html = html.replace('</body>', script + '\n</body>', 1)
html = _stamp_dates(html)
# 3) 构建期把面板挪到 groups 之前
html = move_panel_html(html)
# 4) 原子写盘
_tmp = OUT + '.tmp'
with io.open(_tmp, 'w', encoding='utf-8') as f:
    f.write(html)
os.replace(_tmp, OUT)
print('OK bytes', len(html.encode('utf-8')), 'models', len(canon), 'base', os.path.basename(_base))
