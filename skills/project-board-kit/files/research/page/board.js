
(function(){
  var $=function(s,c){return (c||document).querySelector(s)},$$=function(s,c){return [].slice.call((c||document).querySelectorAll(s))};
  var rm=window.matchMedia&&matchMedia('(prefers-reduced-motion:reduce)').matches;
  var ph='place'+'holder';
  var box=$('#q'); if(box){box.setAttribute(ph,'搜机型 / 客户 / 卡点');}
  /* theme */
  var root=document.documentElement,tt=$('#theme-t');
  try{var sv=localStorage.getItem('frd-theme'); if(sv)root.setAttribute('data-theme',sv);}catch(e){}
  function paint(){if(tt)tt.textContent=root.getAttribute('data-theme')==='dark'?'浅色':'深色';}
  var tb=$('#theme'); if(tb)tb.addEventListener('click',function(){
    var d=root.getAttribute('data-theme')==='dark';
    root.setAttribute('data-theme',d?'light':'dark');
    try{localStorage.setItem('frd-theme',d?'light':'dark');}catch(e){} paint();
  }); paint();
  /* filter cards */
  var P='ALL',S='ALL',O='ALL';
  function apply(){
    var t=(box&&box.value||'').trim().toLowerCase(),n=0;
    $$('.pc').forEach(function(c){
      var ok=(P==='ALL'||c.dataset.prio===P)&&(S==='ALL'||c.dataset.st===S)&&(O==='ALL'||c.dataset.owner===O)&&(!t||(c.dataset.search||'').indexOf(t)>=0);
      c.style.display=ok?'':'none'; if(ok)n++;
    });
    $$('.grp').forEach(function(g){
      var vis=$$('.pc',g).some(function(c){return c.style.display!=='none'});
      g.style.display=vis?'':'none';
    });
    var fc=$('#fc'); if(fc)fc.textContent=n+' / '+$$('.pc').length;
  }
  if(box)box.addEventListener('input',apply);
  $$('.fs').forEach(function(g){
    g.addEventListener('click',function(e){
      var b=e.target.closest('button'); if(!b)return;
      $$('button',g).forEach(function(x){x.classList.remove('on')}); b.classList.add('on');
      if(b.dataset.p!==undefined)P=b.dataset.p;
      if(b.dataset.s!==undefined)S=b.dataset.s;
      if(b.dataset.o!==undefined)O=b.dataset.o;
      apply();
    });
  });
  apply();
  /* drawer */
  var ov=$('#ov'),dw=$('#dw'),dwB=$('#dw-b'),dwT=$('#dw-t'),dwP=$('#dw-p');
  function open(model){
    var d=window.__DRAW&&window.__DRAW[model]; if(!d)return;
    dwT.textContent=d.model;
    dwP.innerHTML='<i class="pr '+d.prio.toLowerCase()+'">'+d.prio+'</i><i class="dot '+(d.stKey||'other')+'"></i><span style="font-size:11.5px;color:var(--ink-2)">'+(d.status||'—')+'</span>';
    var h='';
    h+='<div><div class="dw-k">客户 / 规格</div><div class="dw-v">'+esc(d.brand)+'</div></div>';
    if(d.owner)h+='<div><div class="dw-k">主责</div><div class="dw-v">'+esc(d.owner)+'</div></div>';
    h+='<div><div class="dw-k">卡点</div><div class="dw-v red">'+esc(d.blocker)+'</div></div>';
    h+='<div><div class="dw-k">下一硬节点</div><div class="dw-v">'+esc(d.next)+'</div></div>';
    h+='<div><div class="dw-k">本周动作</div><div class="dw-v">'+esc(d.action)+'</div></div>';
    if(d.note)h+='<div><div class="dw-k">备注</div><div class="dw-v" style="color:var(--ink-2);font-size:11.5px">'+esc(d.note)+'</div></div>';
    if(d.ms&&d.ms.length){
      /* 里程碑：日期归一 -> 状态判定 -> 按计划日期排序。表真值「46344(11/18)」与生成期 M/D 两种来源共用；
         圆点只认日期真相，文档状态原文另列，二者冲突时在 title 里点名。 */
      /*__MIL_LIB__*/
      var MLIST=FRDMIL.list(d.ms);
      var tally={done:0,late:0,over:0,today:0,pend:0,na:0};
      MLIST.rows.forEach(function(r){ tally[r.st]=(tally[r.st]||0)+1; });
      var prog=d.prog||{done:tally.done||0, total:MLIST.rows.length};
      h+='<div><div class="dw-k">已写回 '+(tally.done+tally.late)+' / '+MLIST.rows.length+'（文档登记 '+prog.done+' / '+prog.total+'）</div>'
        +'<div class="dw-lg">'
        +'<span><i class="lg done"></i>已完成 '+tally.done+'</span>'
        +'<span><i class="lg late"></i>延期完成 '+tally.late+'</span>'
        +'<span><i class="lg over"></i>逾期未完成 '+tally.over+'</span>'
        +'<span><i class="lg today"></i>今天到期 '+tally.today+'</span>'
        +'<span><i class="lg pend"></i>未到期 '+tally.pend+'</span>'
        +'<span><i class="lg na"></i>无计划日期 '+tally.na+'</span>'
        +'</div><div class="dw-ml">';
      MLIST.rows.forEach(function(r){
        var when=r.da?('计 '+(MLIST.mmdd(r.dp)||'—')+' · 实 '+MLIST.mmdd(r.da)):(r.dp?('计 '+MLIST.mmdd(r.dp)):'无计划日期');
        if(r.st==='over')when=when+' · 逾期 '+Math.round((Date.parse(MLIST.today)-Date.parse(r.dp))/86400000)+' 天';
        if(r.st==='late')when=when+' · 延 '+Math.round((Date.parse(r.da)-Date.parse(r.dp))/86400000)+' 天';
        var tip=[r.s,r.note,r.conflict].filter(function(x){return x;}).join(' · ');
        h+='<div class="dw-mrow '+r.st+'"'+(tip?(' title="'+esc(tip)+'"'):'')+'>'
          +'<span class="nm">'+esc(r.n)+'</span>'
          +(r.s?('<i class="st">'+esc(r.s+(r.conflict?' ⚠':''))+'</i>'):'')
          +'<i class="when">'+esc(when)+'</i><em></em></div>';
      });
      if(!MLIST.rows.length){h+='<div class="dw-mempty">该机型还没有节点表：目前只有台账 / 预立项汇总里的一行，补进专项进度表后这里会自动出里程碑与状态灯</div>';}
      h+='</div><div class="dw-hint">圆点按日期真相：绿=已写回 · 黄=延期写回 · 红=逾期未写回 · 橙=今天到期 · 空心=未到期或无计划日期；排序按计划日期；悬停看备注与数据冲突</div></div>';
    } else {
      // 台账里有、专项进度表里还没有节点表（如刚预立项）：给出解释而不是空白框
      h+='<div><div class="dw-k">里程碑</div><div class="dw-mempty">该机型还没有节点表：目前只有台账 / 预立项汇总里的一行，补进专项进度表后这里会自动出里程碑与状态灯</div></div>';
    }
    if(d.chatMs&&d.chatMs.length){
      h+='<div><div class="dw-k">群聊口径（未写回文档）</div><div class="dw-cm">';
      d.chatMs.forEach(function(c){ h+='<div class="dw-cl"><b>'+esc(String(c.t||'').slice(5,10))+'</b><span>'+esc(c.n)+'</span><i>约定 '+esc(c.d)+' · '+esc(c.who)+' · '+esc(c.t)+'</i></div>'; });
      h+='</div><div class="dw-hint">左侧是消息日期，「约定」是群里说到的计划日期；确认后我再写回文档</div></div>';
    }
    if(d.priv&&d.priv.length){
      h+='<div><div class="dw-k">私聊印证 '+d.priv.length+' 条</div><div class="dw-cm">';
      d.priv.forEach(function(c){ h+='<div class="dw-ci"><b>'+esc(c.who)+'</b><span>'+esc(c.x)+'</span><i>'+esc(c.t)+'</i></div>'; });
      h+='</div><div class="dw-hint">来自本机企业微信同项目成员的私聊，仅作佐证，不改文档</div></div>';
    }
    h+='<div><div class="dw-k">最新表格更新</div><div class="dw-v" style="color:var(--ink-2);font-size:11.5px">'+esc(d.latest)+'</div></div>';
    dwB.innerHTML=h; dwB.scrollTop=0;
    ov.classList.add('on'); dw.classList.add('on'); dw.setAttribute('aria-hidden','false');
  }
  function close(){ov.classList.remove('on'); dw.classList.remove('on'); dw.setAttribute('aria-hidden','true');}
  document.addEventListener('click',function(e){
    var mr=e.target.closest('.mrow'); if(mr){open(mr.dataset.model); return;}
    var c=e.target.closest('.pc'); if(c){open(c.dataset.model); return;}
    var dr=e.target.closest('.dayrow'); if(dr){open(dr.dataset.model); return;}
    var gd=e.target.closest('.gdot'); if(gd){open(gd.dataset.model); return;}
    var gn=e.target.closest('.grow .gname'); if(gn){open(gn.closest('.grow').dataset.model); return;}
    var lr=e.target.closest('.lrow'); if(lr){closeLm(); open(lr.dataset.model); return;}
    var kp=e.target.closest('[data-kpi]');
    if(kp){ if(openList(kp.dataset.kpi)) return; }
    var st=e.target.closest('[data-scroll]');
    if(st){var t=$(st.dataset.scroll); if(t)t.scrollIntoView({behavior:rm?'auto':'smooth',block:'start'});}
  });
  if(ov)ov.addEventListener('click',close);
  var dx=$('#dw-x'); if(dx)dx.addEventListener('click',close);
  document.addEventListener('keydown',function(e){if(e.key==='Escape'){closeLm();close();}});
  /* 某日节点明细 */
  var dbx=$('#daybox');
  function renderDay(d){
    if(!dbx)return;
    if(!d){dbx.hidden=true;dbx.innerHTML='';return;}
    var list=(window.__DAYD&&window.__DAYD[d])||[];
    dbx.hidden=false;
    if(!list.length){dbx.innerHTML='<div class="dhead">'+d.slice(5)+'</div><div class="dempty">当天没有节点</div>';return;}
    var _ns={};list.forEach(function(x){String(x.n||'').split(' / ').forEach(function(t){if(t)_ns[t]=1})});var nn=Object.keys(_ns).length;var h='<div class="dhead">'+d.slice(5)+' \u00b7 <em>'+list.length+' \u6761 \u00b7 '+(function(){var _s={};list.forEach(function(x){_s[x.model]=1});return Object.keys(_s).length})()+' \u53f0\u673a\u578b \u00b7 \u70b9\u4efb\u4e00\u6761\u770b\u5b8c\u6574\u5361\u70b9</em></div><div class="dlist">';
    list.forEach(function(x){
      var pr=(x.pr||'').toLowerCase();
      var ok=(x.bl&&x.bl!=='\u65e0\u663e\u6027\u5361\u70b9');
      h+='<button class="dayrow'+(x.dup?' dup':'')+'" data-model="'+esc(x.model)+'">'
        +'<i class="pr '+(pr||'p2')+'">'+(x.pr||'-')+'</i>'
        +(x.dup?'<i class="ptag dup-tag">同族</i>':'')+'<span class="dm">'+esc(x.m)+'<em>'+esc(x.k)+' \u00b7 '+esc(x.n)+(x.ow?(' \u00b7 '+esc(x.ow)):'')+'</em></span>'
        +'<span class="db'+(ok?'':' ok')+'">'+(ok?('\u5361\u70b9\uff1a'+esc(x.bl)):'\u65e0\u663e\u6027\u5361\u70b9')+'</span>'
        +'</button>';
    });
    dbx.innerHTML=h+'</div>';
  }
  /* KPI -> 清单弹层 */
  var lm=$('#lm'), lmT=$('#lm-t'), lmC=$('#lm-c'), lmB=$('#lm-b');
  var NONE='\u65e0\u663e\u6027\u5361\u70b9';
  function closeLm(){ if(lm){lm.classList.remove('on');lm.setAttribute('aria-hidden','true');} }
  function openList(kind){
    if(!lm)return;
    var t='',c='',h='';
    if(kind==='nodes'){
      var rows=[];
      Object.keys(window.__DAYD||{}).forEach(function(d){
        if(d<'__TODAYSTR__')return;
        (window.__DAYD[d]||[]).forEach(function(x){rows.push({d:d,x:x});});
      });
      rows.sort(function(a,b){return a.d<b.d?-1:a.d>b.d?1:0;});
      t='\u672a\u6765\u8282\u70b9'; c=rows.length+' \u4e2a';
      h=rows.map(function(r){
        var x=r.x, ok=(x.bl&&x.bl!==NONE);
        return '<button class="lrow" data-model="'+esc(x.model)+'">'
          +'<span class="ld">'+r.d.slice(5)+'</span>'
          +'<b class="lm-m">'+esc(x.m)+'</b>'
          +'<span class="lm-n">'+esc(x.k)+' \u00b7 '+esc(x.n)+(x.ow?(' \u00b7 '+esc(x.ow)):'')+'</span>'
          +'<span class="lb'+(ok?'':' ok')+'">'+(ok?('\u5361\u70b9\uff1a'+esc(x.bl)):NONE)+'</span>'
          +'</button>';
      }).join('');
    } else if(kind==='blocks'||kind==='p0'||kind==='all'){
      var D=window.__DRAW||{}, arr=[];
      Object.keys(D).forEach(function(k){arr.push(D[k]);});
      if(kind==='p0'){arr=arr.filter(function(d){return d.prio==='P0';});t='P0 \u5fc5\u76ef';}
      else if(kind==='all'){t='\u5728\u7ba1\u9879\u76ee';}
      else {arr=arr.filter(function(d){return d.blocker&&d.blocker!==NONE;});t='\u5f85\u89e3\u5361\u70b9';}
      arr.sort(function(a,b){return a.prio<b.prio?-1:a.prio>b.prio?1:0;});
      c=arr.length+(kind==='all'?' \u4e2a':' \u9879');
      h=arr.map(function(d){
        return '<button class="lrow" data-model="'+esc(d.model)+'">'
          +'<span class="ld">'+esc(d.prio)+'</span>'
          +'<b class="lm-m">'+esc(d.model)+'</b>'
          +'<span class="lm-n">'+esc(d.brand)+'</span>'
          +'<span class="lb">\u5361\u70b9\uff1a'+esc(d.blocker)+'</span>'
          +'<span class="lm-n" style="grid-column:2/-1;color:var(--ink-3)">\u4e0b\u4e00\u8282\u70b9\uff1a'+esc(d.next)+'</span>'
          +'</button>';
      }).join('');
    } else { return false; }
    lmT.textContent=t; lmC.textContent=c; lmB.innerHTML=h;
    lm.classList.add('on'); lm.setAttribute('aria-hidden','false');
    return true;
  }
  if(lm)lm.addEventListener('click',function(e){ if(e.target===lm)closeLm(); });
  var lmx=$('#lm-x'); if(lmx)lmx.addEventListener('click',closeLm);
  /* heat -> gantt */
  var curDay='';
  $$('.hcol').forEach(function(h){
    h.addEventListener('click',function(){
      var d=h.dataset.day;
      curDay=(curDay===d)?'':d;
      $$('.hcol').forEach(function(x){x.classList.toggle('on',x.dataset.day===curDay)});
      $$('.ghd').forEach(function(x){
        var on=!!curDay && x.dataset.day===curDay;
        x.classList.toggle('on',on);
        x.classList.toggle('filt',!!curDay);
      });
      var any=false;
      $$('.grow').forEach(function(r){
        if(!curDay){r.classList.remove('hide','hit');return;}
        var hit=r.querySelector('.gdot')?false:false;
        var cells=$$('.gcell',r); var idx=$$('.hcol').findIndex(function(x){return x.dataset.day===curDay});
        var has=cells[idx]&&cells[idx].querySelector('.gdot');
        r.classList.toggle('hide',!has); r.classList.toggle('hit',!!has); if(has)any=true;
      });
      var lb=$('#glab'); if(lb)lb.textContent=curDay?('筛选 '+curDay.slice(5)+(any?'':' · 无项目')):'全部日期';
      renderDay(curDay);
    });
  });
  $$('.ghd').forEach(function(h){
    h.addEventListener('click',function(){
      var i=+h.dataset.i; var cols=$$('.hcol'); if(cols[i]) cols[i].click();
    });
  });
  var gc=$('#gclear'); if(gc)gc.addEventListener('click',function(){
    curDay=''; $$('.hcol').forEach(function(x){x.classList.remove('on')});
    $$('.ghd').forEach(function(x){x.classList.remove('on','filt')});
    $$('.grow').forEach(function(r){r.classList.remove('hide','hit')});
    var lb=$('#glab'); if(lb)lb.textContent='全部日期';
    renderDay('');
  });
  /* ---------- motion：GSAP 4 beat，克制；rm 或 gsap 缺失即终态 ---------- */
  var gs=(typeof window.gsap!=='undefined')?window.gsap:null;
  if(gs&&!rm){
    var TL=gs.timeline({defaults:{ease:'power2.out'}});
    $$('[data-n]').forEach(function(el){
      var tv=parseFloat(el.dataset.n), node=el.firstChild;
      if(!isFinite(tv)||tv<=0||!node||node.nodeType!==3)return;
      var o={v:0};
      TL.to(o,{v:tv,duration:.8,ease:'power2.out',onUpdate:function(){node.nodeValue=String(Math.round(o.v))},onComplete:function(){node.nodeValue=String(Math.round(tv))}},0);
    });
    $$('.donut').forEach(function(d){
      var o={a:0};
      TL.to(o,{a:360,duration:1.0,ease:'power2.inOut',onUpdate:function(){d.style.setProperty('--a',o.a+'deg')}},0.02);
    });
    if($$('.gtrack').length)TL.from('.gtrack',{scaleX:0,duration:.66,ease:'power3.out',stagger:.012},0.08);
    if('IntersectionObserver' in window){
      var io=new IntersectionObserver(function(es){
        es.forEach(function(en){
          if(!en.isIntersecting)return;
          io.unobserve(en.target);
          var items=$$('.pc',en.target);
          if(items.length)gs.fromTo(items,{y:9,opacity:0},{y:0,opacity:1,duration:.4,ease:'power2.out',stagger:.012,overwrite:true});
        });
      },{rootMargin:'0px 0px -6% 0px'});
      $$('.grp').forEach(function(g){io.observe(g)});
    }
  }
  /* 抽屉数据 + esc */
  window.__DRAW=__DRAW__;
  window.__DAYD=__DAYD_DATA__;
  function esc(s){return String(s==null?'':s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');}
})();
