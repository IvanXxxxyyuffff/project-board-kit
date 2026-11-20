r"""把 V4 看板里用 conic-gradient 画的环形图（.donut）在浏览器端重写成 SVG 描边环。

这是一次性注入脚本的「带注释母版」：读 项目数据看板.html，先删掉上一版注入的
<script id="ring-svg-fix2?">，再把自带 why 的修复杂进 </body> 前。captain 稍后会
把这段合进 inject_sheet_truth.py，因此这里只补可读性注释，不动任何逻辑/正则/id。

为什么要重写：conic-gradient 在高 DPI / 打印 / 部分 WebView 里扇形边界会发虚、
每段颜色靠角度硬切、没法单独给每段留缝或做 hover；改成 SVG circle +
stroke-dasharray 后每段是独立矢量圆弧，角度与颜色由 CSS 变量解析出来一一对应，
缩放不糊、缝隙可控。脚本在浏览器里跑，Python 侧只负责字符串注入。
"""
import io, re, os
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
p = os.environ.get('V4_OUT') or os.path.join(ROOT, '项目数据看板.html')
h = io.open(p, encoding='utf-8').read()
# 幂等前提：先把旧版 ring-svg-fix2? 整段删干净再注入，否则重复注入会叠加多个
# SVG 环与多份监听器；这一删是 5a 幂等验收的前置条件，正则/id 均不得改。
h = re.sub(r'<script id="ring-svg-fix2?">.*?</script>\s*', '', h, flags=re.S)

script = r'''<script id="ring-svg-fix2">(function(){
function parse(bg){
  // 弃用 conic-gradient 改 SVG stroke-dasharray：把 CSS 里的 from 起始角与
  // 各段「颜色 起角 终角」解析成 segs，交给 <circle> 的 dash 逐段描边。
  var i=bg.indexOf('conic-gradient'); if(i<0) return null;
  var body=bg.slice(bg.indexOf('(',i)+1, bg.lastIndexOf(')'));
  var from=0, m=body.match(/from\s+(-?[\d.]+)(deg|turn|rad)/);
  if(m){ var v=parseFloat(m[1]); from = m[2]==='turn'? v*360 : (m[2]==='rad'? v*180/Math.PI : v); body=body.replace(m[0],''); }
  var parts=[],depth=0,cur='';
  for(var k=0;k<body.length;k++){var ch=body[k];
    if(ch==='(')depth++;
    if(ch===')')depth--;
    if(ch===','&&depth===0){parts.push(cur);cur='';}else cur+=ch;}
  if(cur.trim())parts.push(cur);
  var raw=[];
  parts.forEach(function(s){
    s=s.trim(); if(!s)return;
    var mm=s.match(/^(.+?)\s+(-?[\d.]+)(deg|%)?\s+(-?[\d.]+)(deg|%)?\s*$/);
    if(mm){
      var p1=parseFloat(mm[2])*(mm[3]==='%'?3.6:1), p2=parseFloat(mm[4])*(mm[5]==='%'?3.6:1);
      raw.push({col:mm[1].trim(), a:p1, b:p2}); return;
    }
    var m1=s.match(/^(.+?)\s+(-?[\d.]+)(deg|%)?\s*$/);
    if(m1){ raw.push({col:m1[1].trim(), a:parseFloat(m1[2])*(m1[3]==='%'?3.6:1), b:null}); }
  });
  for(var q=0;q<raw.length;q++){
    if(raw[q].a==null) raw[q].a = q? raw[q-1].b : 0;
    if(raw[q].b==null) raw[q].b = (q+1<raw.length ? raw[q+1].a : 360);
  }
  return {from:from, segs:raw.map(function(r){return [r.col, r.a, r.b];})};
}
function fix(){
  var ds=document.querySelectorAll('.donut');
  for(var i=0;i<ds.length;i++){
    var d=ds[i]; if(d.getAttribute('data-ring')==='1') continue;
    var cs=getComputedStyle(d);
    var g=parse(cs.backgroundImage||'');
    // r=53.5、viewBox/宽高 118：圆心 59,59 加半径 53.5 再留 11 的描边半宽，
    // 刚好填满 118x118 不裁切；C=2πr 是整圈周长，dash 长度按 span/360*C 换算。
    var C=2*Math.PI*53.5;
    var run=cs.getPropertyValue('--run').trim()||'#41BA52';
    var list=(g&&g.segs.length)?g.segs:[[run,0,360]];
    var rot0=g?g.from:0;
    d.style.background='none';
    var v=d.querySelector('.veil'); if(v) v.style.display='none';
    var svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('viewBox','0 0 118 118'); svg.setAttribute('width','118'); svg.setAttribute('height','118');
    svg.style.cssText='position:absolute;inset:0;pointer-events:none;z-index:1';
    var inner='<circle cx="59" cy="59" r="53.5" fill="none" stroke="rgba(232,234,240,.10)" stroke-width="11"/>';
    list.forEach(function(s){
      var col=s[0], a0=s[1], a1=s[2];
      var span=a1-a0; if(span<0) span+=360; if(span<=0.05) return;
      var len=C*span/360;
      // dash 减 0.45：相邻色段之间留一道肉眼可见的缝，避免两段颜色首尾糊在一起；
      // 整圈(>=359.9)时不减，否则会露出缺口。
      var dash=(span>=359.9? C : Math.max(0.1,len-0.45)).toFixed(2)+' '+C.toFixed(2);
      // rotate(a0+rot0)：SVG circle 默认从 3 点方向起画，加上该段起角 a0 与
      // conic-gradient 的 from 起始角 rot0，才能还原 CSS 里的真实朝向。
      inner+='<circle cx="59" cy="59" r="53.5" fill="none" stroke="'+col+'" stroke-width="11" stroke-dasharray="'+dash+'" transform="rotate('+(a0+rot0).toFixed(3)+' 59 59)"/>';
    });
    svg.innerHTML=inner;
    d.insertBefore(svg,d.firstChild);
    // data-ring='1' 幂等标记：晚到的时机回调再跑 fix() 时跳过已处理过的 .donut，
    // 不会重复插 SVG。
    d.setAttribute('data-ring','1');
  }
}
// 三重时机覆盖异步晚渲染：立即 fix() 抓已存在的 DOM；DOMContentLoaded 抓文档
// 解析完才挂上的环；setTimeout(500/1500) 兜住看板 JS 异步拼装/延迟渲染的 .donut。
fix(); document.addEventListener('DOMContentLoaded',fix); setTimeout(fix,500); setTimeout(fix,1500);
})();</script>'''

h = h.replace('</body>', script + '\n</body>', 1)
_t = p + '.tmp'; io.open(_t, 'w', encoding='utf-8', newline='').write(h); os.replace(_t, p)
print('ring v3 injected', len(h))
