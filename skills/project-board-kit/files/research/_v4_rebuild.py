# -*- coding: utf-8 -*-
"""同版式重渲染：把「你的 V4 外壳」和「当轮数据 body」拼成候选页。

问题（用户 2026-11-16 反馈）：`项目数据看板.html` 的 01/03/04/05 等区块是 11-13 的
预渲染快照，注入脚本只改品牌分布/归档/圆环，所以时间轴仍从 13 号起、群聊没有 16 号消息。

做法：head（CSS/字体/主题，决定观感）取现役 V4 页；body（数据区 DOM）取当轮
`最新项目简报.html`。两者由同一个 build_daily_brief.mjs 产出、class 体系一致，
所以拼接后观感不变、数据是当轮。

安全：默认只写 `research/_v4_cand.html` 供截图比对；加 `--inplace` 才写回现役
`项目数据看板.html`（覆盖前留 `research/_v4_backup.html`，并跑 inject_sheet_truth.py
与 _kpi_pop.py 补齐结构与样式）。
"""
import io, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, '项目数据看板.html')          # 现役 V4 页（只读）
GEN = os.path.join(ROOT, '最新项目简报.html')           # 当轮生成页（只读）
CAND = os.path.join(ROOT, 'research', '_v4_cand.html')  # 候选页（可写）

v4 = io.open(OUT, encoding='utf-8').read()
gen = io.open(GEN, encoding='utf-8').read()

iv = v4.find('<body')
ig = gen.find('<body')
assert iv > 0 and ig > 0, 'missing <body>'

head = v4[:iv]            # V4 的 <head>：CSS/字体/主题，观感来源
body = gen[ig:]           # 当轮 body：数据区 DOM
cand = head + body

# 表头联动样式：head 取自现役 V4 页，不会带上生成器新增的 CSS（.ghd.on 等），
# 所以在这里按 id 幂等注入；否则"点 25 号，16 号还亮"的观感问题会复现。
LINK_CSS = (
    '<style id="gantt-link-css">'
    '.ghd.on{background:rgba(255,194,71,.20);color:#FFC247;font-weight:700;border-radius:6px;'
    'box-shadow:inset 0 0 0 1px rgba(255,194,71,.50)}'
    '.ghd.now{color:#FFC247;font-weight:700}'
    '.ghd.now.filt{color:rgba(232,234,240,.34);font-weight:400}'
    '.gdot.dup{background:transparent!important;box-shadow:inset 0 0 0 1px rgba(255,194,71,.6)!important;opacity:.60}'
    '</style>'
)
# 群聊两列 + 里程碑状态色：body 由 build_daily_brief.mjs 产出、class 已就位，
# 但 head 取现役 V4 页，带的是老规则（.feed li 四列、li:nth-child(n+13) 隐藏、
# .dw-mi 三列），所以同样按 id 幂等注入，覆盖成当轮口径。
CHAT_CSS = (
    '<style id="chat-ms-css">'
    '.mfold{margin-top:6px}'
    '.mfold>summary{cursor:pointer;font-size:11.5px;color:var(--ink-3);padding:4px 2px;list-style:none}'
    '.mfold>summary::-webkit-details-marker{display:none}'
    '.mfold>summary:before{content:"\\25B8  ";color:var(--acc)}'
    '.mfold[open]>summary:before{content:"\\25BE  "}'
    '.mfold>summary:focus-visible{outline:2px solid var(--acc);outline-offset:2px}'
    '.fcols{display:grid;grid-template-columns:1fr 1fr;column-gap:34px}'
    '@media(max-width:1000px){.fcols{grid-template-columns:1fr;row-gap:14px}}'
    '.fcol{min-width:0;overflow:hidden}'
    '.fch{display:flex;align-items:center;gap:6px;font-size:10px;color:var(--ink-3);'
    'padding:0 2px 6px;margin-bottom:2px;border-bottom:1px solid var(--line-2)}'
    '.feed ul{display:block!important;max-height:none!important;overflow:visible!important;'
    'grid-auto-flow:row!important;grid-template-columns:none!important}'
    '.feed li{display:grid!important;grid-template-columns:38px 38px 70px 1fr!important;'
    'gap:8px;align-items:baseline;flex-wrap:nowrap!important;padding-right:10px}'
    '.feed li:nth-child(n+13){display:grid!important}'
    '.feed li .ptag{grid-column:2}'
    '.feed li b{grid-column:3;max-width:64px}'
    '.feed li span{grid-column:4;display:block!important;-webkit-line-clamp:unset!important;'
    'white-space:nowrap;overflow:hidden;text-overflow:ellipsis}'
    '.feed li .src{display:none!important}'
    '.ptag{white-space:nowrap}'
    '.ptag.em{background:var(--track);color:var(--ink-3)}'
    '.ptag.wx{background:var(--acc-soft);color:var(--acc)}'
    '.dw-lg{display:flex;flex-wrap:wrap;gap:8px 10px;font-size:10px;color:var(--ink-3);margin:2px 0 7px}'
    '.dw-lg span{display:inline-flex;align-items:center;white-space:nowrap}'
    '.dw-lg i.lg{display:inline-block;width:6px;height:6px;border-radius:50%;background:var(--muted-2);margin-right:4px;flex:none}'
    '.dw-lg i.lg.done{background:var(--run)}'
    '.dw-lg i.lg.late{background:var(--p1)}'
    '.dw-lg i.lg.over{background:var(--p0)}'
    '.dw-lg i.lg.today{background:var(--p1);box-shadow:0 0 0 2px var(--acc-soft)}'
    '.dw-lg i.lg.pend{background:transparent;box-shadow:inset 0 0 0 1px var(--line-2)}'
    '.dw-lg i.lg.na{background:transparent;box-shadow:inset 0 0 0 1px var(--line-2);opacity:.55}'
    '.dw-mempty{font-size:11.5px;line-height:1.65;color:var(--ink-3);background:var(--track);border-radius:6px;padding:8px 10px}'
    '.dw-ml{display:flex;flex-direction:column}'
    '.dw-mrow{display:grid;grid-template-columns:minmax(0,1fr) auto auto 8px;gap:9px;align-items:center;font-size:11px;padding:4px 0;border-bottom:1px solid var(--line-2)}'
    '.dw-mrow:last-child{border-bottom:0}'
    '.dw-mrow .nm{color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}'
    '.dw-mrow .st{font-style:normal;font-size:10px;color:var(--ink-3);max-width:96px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}'
    '.dw-mrow .when{font-style:normal;font-family:var(--mono);font-size:10px;color:var(--ink-4);white-space:nowrap}'
    '.dw-mrow.late .when{color:var(--p1)}'
    '.dw-mrow.over .when{color:var(--p0)}'
    '.dw-mrow em{font-style:normal;width:6px;height:6px;border-radius:50%;background:var(--muted-2);justify-self:center}'
    '.dw-mrow.done em{background:var(--run)}'
    '.dw-mrow.late em{background:var(--p1)}'
    '.dw-mrow.today em{background:var(--p1);box-shadow:0 0 0 3px var(--acc-soft)}'
    '.dw-mrow.over em{background:var(--p0);box-shadow:0 0 0 3px rgba(255,107,97,.14)}'
    '.dw-mrow.na em{background:transparent;box-shadow:inset 0 0 0 1px var(--line-2)}'
    '</style>'
)
cand = re.sub(r'<style id="gantt-link-css">.*?</style>\s*', '', cand, flags=re.S)
cand = re.sub(r'<style id="chat-ms-css">.*?</style>\s*', '', cand, flags=re.S)
assert '</head>' in cand, 'missing </head>'
cand = cand.replace('</head>', LINK_CSS + CHAT_CSS + '\n</head>', 1)

INPLACE = '--inplace' in sys.argv
target = OUT if INPLACE else CAND

if INPLACE:
    # 覆盖前留一份可回滚副本（同目录、带日期，不进清理白名单但手工可恢复）
    bak = os.path.join(ROOT, 'research', '_v4_backup.html')  # 固定名，每次覆盖，不留时间戳堆垃圾
    io.open(bak, 'w', encoding='utf-8', newline='').write(v4)
    print('BACKUP %s' % bak)

io.open(target, 'w', encoding='utf-8', newline='').write(cand)
print('CAND head=%d body=%d total=%d target=%s' % (len(head), len(body), len(cand), os.path.basename(target)))

# 在目标页上跑一遍注入 + 圆环（非 --inplace 时写 CAND，不动现役）
env = dict(os.environ, V4_OUT=target)
for fn in ('inject_sheet_truth.py', '_ring_fix3.py', '_kpi_pop.py'):
    r = subprocess.run([sys.executable, '-X', 'utf8', os.path.join(ROOT, 'research', fn)],
                       env=env, capture_output=True, text=True, encoding='utf-8', errors='replace')
    print('[%s] rc=%d %s' % (fn, r.returncode, (r.stdout or '').strip()[-160:]))
    if r.returncode != 0:
        print((r.stderr or '')[-400:])
        sys.exit(1)

h = io.open(target, encoding='utf-8').read()
print('FINAL bytes=%d title=%s' % (len(h.encode('utf-8')), h[h.find('<title>') + 7:h.find('</title>')]))
