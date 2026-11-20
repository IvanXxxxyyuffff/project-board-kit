# -*- coding: utf-8 -*-
"""01「今天必须处理」改成卡片→弹窗（用户 2026-11-16 指定：新版的明细是隐藏的，点卡片才弹窗）。

做法：
1. 注入 `<style id="kpi-pop-css">`：默认隐藏原来的四列大面板 `.panel.must`，
   只留 4 个 KPI 卡；卡片下方给一行「展开全部明细」小按钮，可回退到旧视图。
2. 注入 `<script id="kpi-pop-js">`：点 `.ugrid .ug` → 按卡片文案匹配 `.msec` 分块，
   把它克隆进浮层 `#kp`（标题 + 条数 + 关闭）；点浮层里的行，转发给页面原有 `.mrow` 处理器。
3. 幂等：每次先按 id 精确删掉旧 CSS/JS/浮层节点再插回，可反复跑。

依赖：`项目数据看板.html`（已重渲染 + 注入 sheet-truth）。路径相对本文件。
"""
import io, os, re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.environ.get('V4_OUT') or os.path.join(ROOT, '项目数据看板.html')

CSS = r'''<style id="kpi-pop-css">
/* 新版：明细默认隐藏，点 KPI 卡才弹窗 */
.panel.must{display:none}
#kp-more{display:inline-flex;align-items:center;gap:6px;margin:2px 0 0 2px;padding:5px 11px;border:1px solid var(--line);
  border-radius:999px;background:transparent;color:var(--ink-3);font:600 11px/1 var(--sans);cursor:pointer;transition:.16s}
#kp-more:hover{color:var(--ink);border-color:var(--line-2)}
body.kp-open .panel.must{display:block}
#kp{position:fixed;inset:0;z-index:90;display:none;align-items:center;justify-content:center;
  background:rgba(6,8,12,.62);backdrop-filter:blur(3px)}
#kp.on{display:flex}
#kp-c{width:min(680px,92vw);max-height:82vh;display:flex;flex-direction:column;background:var(--surf);
  border:1px solid var(--line);border-radius:14px;box-shadow:0 24px 64px rgba(0,0,0,.55);overflow:hidden}
#kp-h{display:flex;align-items:center;gap:10px;padding:14px 16px;border-bottom:1px solid var(--line);font:600 13px/1 var(--sans)}
#kp-h b{font-family:var(--mono);font-size:12px;color:var(--ink-3)}
#kp-x{margin-left:auto;border:0;background:none;color:var(--ink-3);font-size:18px;line-height:1;cursor:pointer;padding:2px 4px}
#kp-x:hover{color:var(--ink)}
#kp-b{padding:8px 10px 12px;overflow:auto}
#kp-b .mrow{width:100%}
</style>'''

JS = r'''<script id="kpi-pop-js">
(function(){
  if(window.__kpFix) return; window.__kpFix = 1;
  var sec = document.getElementById('sec-must'); if(!sec) return;  // 明细面板 .panel.must 与 #sec-must 非父子，必须全局取

  var more = document.createElement('button');
  more.id='kp-more'; more.type='button'; more.textContent='展开全部明细';
  var grid = sec.querySelector('.ugrid');
  if(grid && grid.parentNode) grid.parentNode.insertBefore(more, grid.nextSibling);

  var pop = document.createElement('div'); pop.id='kp';
  pop.innerHTML = '<div id="kp-c" role="dialog" aria-modal="true">'+
    '<div id="kp-h"><span id="kp-t"></span><b id="kp-n"></b><button id="kp-x" aria-label="关闭">×</button></div>'+
    '<div id="kp-b"></div></div>';
  document.body.appendChild(pop);

  function blocks(){
    var p = document.querySelector('.panel.must'); if(!p) return [];
    return [].slice.call(p.querySelectorAll('.msec'));
  }
  function pick(label){
    var bs = blocks();
    for(var i=0;i<bs.length;i++){
      var h = bs[i].querySelector('.mh'); if(!h) continue;
      var t = h.textContent.replace(/\s+/g,'');
      if(label === 'P0卡点' && /P0/.test(t)) return bs[i];
      if(t.indexOf(label) === 0) return bs[i];
    }
    return null;
  }
  function openBox(sec2){
    var h = sec2.querySelector('.mh');
    var title = h ? h.textContent.replace(/\d+\s*$/,'').trim() : '';
    var rows = sec2.querySelectorAll('.mrow');
    var b = document.getElementById('kp-b');
    b.innerHTML = '';
    for(var i=0;i<rows.length;i++) b.appendChild(rows[i].cloneNode(true));
    document.getElementById('kp-t').textContent = title;
    document.getElementById('kp-n').textContent = rows.length + ' 项';
    pop.classList.add('on');
  }
  function close(){ pop.classList.remove('on'); }

  document.addEventListener('click', function(e){
    var ug = e.target.closest && e.target.closest('.ugrid .ug');
    if(ug){
      var lb = ug.querySelector('.lb');
      var label = (lb ? lb.textContent : '').trim();
      var box = pick(label.replace(/\s+/g,''));
      if(box){ e.preventDefault(); e.stopPropagation(); openBox(box); return; }
      return; // 没有对应分块 → 保留原行为（滚动）
    }
    if(e.target.id === 'kp-more'){ document.body.classList.toggle('kp-open');
      more.textContent = document.body.classList.contains('kp-open') ? '收起全部明细' : '展开全部明细'; return; }
    if(e.target.id === 'kp-x' || e.target.id === 'kp'){ close(); return; }
    var row = e.target.closest && e.target.closest('#kp-b .mrow');
    if(row){
      var model = row.getAttribute('data-model');
      var src = document.querySelector('.panel.must .mrow[data-model="'+model+'"]') ||
                document.querySelector('.mrow[data-model="'+model+'"]');
      close();
      if(src) src.click();
    }
  }, true);

  document.addEventListener('keydown', function(e){ if(e.key === 'Escape') close(); });
})();
</script>'''

html = io.open(OUT, encoding='utf-8').read()

# 幂等清理
html = re.sub(r'<style id="kpi-pop-css">.*?</style>\s*', '', html, flags=re.S)
html = re.sub(r'<script id="kpi-pop-js">.*?</script>\s*', '', html, flags=re.S)

assert html.count('</body>') == 1, 'expect single </body>'
html = html.replace('</body>', CSS + '\n' + JS + '\n</body>', 1)

tmp = OUT + '.tmp'
io.open(tmp, 'w', encoding='utf-8', newline='').write(html)
os.replace(tmp, OUT)
print('KPI-POP ok bytes=%d css=%d js=%d' % (len(html.encode('utf-8')), 1, 1))
