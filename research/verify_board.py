# -*- coding: utf-8 -*-
"""看板静态检测治具 —— 断言全部作用于 HTML 本体，含负例自检。

用法: python verify_board.py [html_path]
退出码 0 = 全绿（含负例能正确 FAIL）。

2026-11-14 审核修复：旧版 7 项里 5 项只读 sheet_canon.json，把 HTML 换成
「260KB 占位垃圾 + 字符串 __DRAW」仍 7/7 PASS（假绿）；circles/ring_colors/
modal/dates 算完从不进 check()。本版把每条结论都绑到 HTML 文本，并从 HTML
里反解注入的 SHEET 数据岛与 canon 对账；另加两组负例，确保断言真会 FAIL。
路径相对 __file__ 上溯两级，分发到同事机后无需改盘符。
"""
import io, json, os, re, sys, datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HTML = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, '项目数据看板.html')
CANON = os.path.join(ROOT, 'research', 'sheet_canon.json')


def collect(html, canon):
    """对 (html, canon) 跑全部断言，返回 [(pass, name, detail)]。"""
    rows = []
    def check(name, cond, detail=''):
        rows.append((bool(cond), name, str(detail)))

    b = html.encode('utf-8')
    # ---------- 1 页面本体（不再只看一个 __DRAW 字符串） ----------
    check('页面 >120KB（V4+注入）', len(b) > 120000, '%d bytes' % len(b))
    check('含 __DRAW 数据岛', 'window.__DRAW' in html or '__DRAW=' in html or '__DRAW =' in html)
    check('含项目卡 .pc', len(re.findall(r'class="[^"]*\bpc\b', html)) >= 8,
          'pc=%d' % len(re.findall(r'class="[^"]*\bpc\b', html)))
    check('含时间轴 / 群聊段', 'sec-nodes' in html and 'id="groups"' in html)

    # ---------- 2 注入块完整性（5a 真的跑过） ----------
    for tag, pat in [
        ('品牌面板 st-wrap', r'<div id="st-wrap"'),
        ('样式 sheet-truth-css', r'<style id="sheet-truth-css">'),
        ('脚本 sheet-truth', r'<script id="sheet-truth">'),
        ('归档浮层 st-modal', r'<div id="st-modal"'),
        ('归档列表 st-list', r'id="st-list"'),
        ('圆环脚本 ring-svg-fix', r'<script id="ring-svg-fix2?">'),
    ]:
        check('HTML 含 ' + tag, re.search(pat, html) is not None)

    # ---------- 3 响应式没被跨删（blocker#1 回归断言） ----------
    check('含 @media(max-width:900px)', '@media(max-width:900px)' in html)
    check('st-wrap 为 grid 双列', 'grid-template-columns:1.15fr 1fr' in html)
    check('双列面板 2 个 stc 段', len(re.findall(r'<section class="stc">', html)) >= 2,
          'stc=%d' % len(re.findall(r'<section class="stc">', html)))
    check('无残留 st-search-fix（搜索已合并）', 'id="st-search-fix"' not in html)
    # 2026-11-16：CSS 在 V4 head、JS 在生成 body，曾出现"JS 落了、CSS 没落"导致
    # 点 25 号甘特表头里的 16 号还亮；这两条把两侧绑一起断言。
    check('含表头联动样式 gantt-link-css', 'gantt-link-css' in html and '.ghd.on{' in html)
    check('含表头联动脚本 filt', "toggle('filt'" in html)

    # ---------- 4 从 HTML 反解 SHEET 数据岛，与 canon 对账 ----------
    m = re.search(r'var SHEET = (\{.*?\});', html, re.S)
    sheet = None
    try:
        if m:
            sheet = json.loads(m.group(1))
    except Exception as e:
        check('SHEET 数据岛可解析', False, str(e)[:80])
    if sheet is None:
        check('SHEET 数据岛可解析', False, 'not found / bad json')
        check('SHEET 机型数 == canon', False)
        models = {}
    else:
        check('SHEET 数据岛可解析', True, '%d keys' % len(sheet))
        models = sheet
        check('SHEET 机型数 == canon', len(sheet) == len(canon), '%d vs %d' % (len(sheet), len(canon)))

    groups = {}
    for v in models.values():
        g = v.get('brandGroup', '')
        groups[g] = groups.get(g, 0) + 1
    check('品牌分组含 019', '019' in groups, str(sorted(groups)))
    check('分组数 ≥6', len(groups) >= 6, str(sorted(groups.items())))
    g019 = [k for k, v in models.items() if v.get('brandGroup') == '019']
    check('019 组含 ITEM_X/ITEM_X',
          any(k.startswith('ITEM_X') for k in g019) and any(k.startswith('ITEM_X') for k in g019), str(g019))

    # ---------- 5 归档（读 HTML 内 SHEET，不再只读 canon 文件） ----------
    arch = {k: v for k, v in models.items() if v.get('status') == '已结案'}
    dated = {k: v for k, v in arch.items() if (v.get('archivedAt') or '').count('-') == 2}
    check('已归档 ≥60 台', len(arch) >= 60, '%d' % len(arch))
    check('归档日期带年份 ≥60 条', len(dated) >= 60, '%d' % len(dated))
    # 浮层 / 内联列表里确实渲染了带年份日期（脚本模板里有 archivedAt 拼装）
    check('HTML 含归档日期渲染', '#st-list' in html and 'archivedAt' in html)

    # ---------- 6 canon 文件本身与 HTML 一致（防两者脱节） ----------
    check('canon 机型数 == SHEET', len(canon) == len(models), '%d vs %d' % (len(canon), len(models)))
    return rows


def failed(rows):
    return sum(1 for good, _, _ in rows if not good)


def doc_checks(html_len):
    """文档口径与现役产物一致性（防止改了体积/调度忘了改说明）。"""
    out = []
    def add(name, cond, note=''):
        out.append((bool(cond), name, note))
    paths = [os.path.join(ROOT, '每日简报_使用说明.md')]
    # 套件自 v2.23 起不再随包带说明书（说明只留项目根一份）；存在才校验，缺失不算失败
    _kitdoc = os.path.join(ROOT, 'skills', 'project-board-kit', 'files', '每日简报_使用说明.md')
    if os.path.exists(_kitdoc):
        paths.append(_kitdoc)
    txts = {}
    for p in paths:
        try:
            txts[p] = io.open(p, encoding='utf-8').read()
        except Exception as e:
            txts[p] = ''
            add('文档可读 %s' % os.path.basename(os.path.dirname(p)), False, str(e))
    for p, t in txts.items():
        tag = 'root' if os.path.dirname(p) == ROOT else 'kit'
        if not t:
            continue
        add('文档 %s 已写「计划任务已禁用」' % tag, '已禁用' in t, '')
        _stale = [ln for ln in t.splitlines()
                  if '工作日 09:05' in ln and not ln.lstrip().startswith('>')]
        add('文档 %s 无「09:05 定时」活跃口径（修订历史段可保留）' % tag, not _stale, '命中=%d' % len(_stale))
        add('文档 %s 写了 exe 内置调度 30/60' % tag, ('30' in t and '60' in t), '')
        _m = re.search(r'\*\*([\d.]+) KB\*\*', t)
        _ok = False
        if _m:
            try:
                _ok = abs(float(_m.group(1)) - html_len / 1024.0) / max(html_len / 1024.0, 1) <= 0.25
            except ValueError:
                _ok = False
        add('文档 %s 看板体积与现役相差 ≤25%%' % tag, _ok, 'doc=%s actual=%.1fKB' % (_m.group(1) if _m else 'NA', html_len / 1024.0))
        add('文档 %s 写了 CLIENT_A 双层闸门' % tag,
            ('client_send_approval.json' in t) and ('-Approved' in t), '')
        add('文档 %s 写了归档自动保留 2 天' % tag, '只保留最近 2 天' in t, '')
    return out


def data_checks(html):
    """数据新鲜度与去重断言（2026-11-16 加）：
    ① 群聊最新时间必须等于今天 —— 历史拉取按游标往回翻，天然滞后，
       曾出现"看板全是 11-13 的消息"而所有旧断言照绿的假绿。
    ② 私聊会话不能为空 —— 私聊按项目关键词打分过滤，曾整体为 []。
    ③ __DAYD 同一天内不能出现"同机型族 + 同节点名"两条 ——
       ITEM_X 与 ITEM_X 是同一支工装，会被重复计成两个节点。
    """
    rows = []
    today = datetime.datetime.now().strftime('%Y-%m-%d')
    em = os.path.join(ROOT, 'research', 'raw', 'emobile')
    try:
        cl_path = os.path.join(em, 'project_chatlog.json')
        g = json.load(io.open(cl_path, encoding='utf-8'))
        newest = max([str(x.get('newest') or '') for x in g] or [''])
        # 2026-11-17 修：早班 09:10 群里还没人发言时，「最新消息 = 昨天傍晚」是真实状态，
        # 不是抓取滞后（当日实测：客户端 conversations_latest 的 max lasttime 同为 11-16 20:17）。
        # 判定：① 最新消息是今天 → 绿；② 否则要求「拉取文件今天重写过」且「最新消息在 36h 内」才放行，
        # 这样「看板全是 11-13 消息」这类真滞后仍然会红。
        fresh_pull = datetime.date.fromtimestamp(os.path.getmtime(cl_path)).isoformat() == today
        recent = False
        try:
            dt_new = datetime.datetime.strptime(newest, '%Y-%m-%d %H:%M')
            recent = (datetime.datetime.now() - dt_new).total_seconds() <= 36 * 3600
        except Exception:
            recent = False
        ok = newest[:10] == today or (fresh_pull and recent)
        note = newest or '—'
        if newest[:10] != today:
            note += '（今日暂无新消息；拉取文件 %s %s，36h 内=%s）' % (
                'today' if fresh_pull else 'NOT-today', datetime.datetime.fromtimestamp(os.path.getmtime(cl_path)).strftime('%H:%M'), recent)
        rows.append((ok, '群聊最新 == 今天', note))
    except Exception as e:
        rows.append((False, '群聊最新 == 今天', 'read fail %s' % str(e)[:60]))
    try:
        pv = json.load(io.open(os.path.join(em, 'personal_chatlog.json'), encoding='utf-8'))
        rows.append((isinstance(pv, list) and len(pv) > 0, '私聊会话非空',
                     '%d convs' % (len(pv) if isinstance(pv, list) else -1)))
    except Exception as e:
        rows.append((False, '私聊会话非空', 'read fail %s' % str(e)[:60]))
    try:
        m = re.search(r'__DAYD\s*=\s*(\{.*?\});', html, re.S)
        dd = json.loads(m.group(1)) if m else {}
        bad = []
        for day, items in dd.items():
            seen = {}
            for it in (items or []):
                fam = (re.search(r'\d{4,5}', str(it.get('model') or '')) or [str(it.get('model'))])[0]
                key = fam + '|' + str(it.get('n') or '')
                seen.setdefault(key, []).append(bool(it.get('dup')))
            for k, flags in seen.items():
                # 同族同节点：允许「1 条主行 + 若干 dup 行」，不允许两条主行
                if flags.count(False) > 1:
                    bad.append('%s %s 主行x%d' % (day, k, flags.count(False)))
        rows.append((not bad, '__DAYD 同族同节点只有一条主行', ('; '.join(bad[:3]) if bad else '%d days' % len(dd))))
    except Exception as e:
        rows.append((False, '__DAYD 同族同节点只有一条主行', 'parse fail %s' % str(e)[:60]))
    return rows


def no_old_gen():
    """防复发：旧的「老 UI」中间页不得重现；归档只能有今天一份。"""
    out = []
    gen = os.path.join(ROOT, '项目数据看板_生成.html')
    out.append((not os.path.exists(gen), '无老 UI 中间页(项目数据看板_生成.html)', ''))
    today = datetime.date.today().isoformat()
    yest = (datetime.date.today() - datetime.timedelta(days=1)).isoformat()
    arch = [f for f in os.listdir(ROOT) if f.startswith('每日简报_') and f.endswith('.html')]
    # 2026-11-17 修：构建器与文档（root/kit）都写「归档自动保留 2 天（含今天）」，
    # build_daily_brief.mjs 也是 arch.slice(2) 保留两份；旧断言只认今天，天天误红。
    stale = [f for f in arch if today not in f and yest not in f]
    out.append((not stale, '日报归档无历史残留(今天+昨天)', ','.join(stale) or 'keep=%s' % ','.join(sorted(f for f in arch if f not in stale))))
    return out


# ---------- 正式断言 ----------
html = io.open(HTML, encoding='utf-8', errors='replace').read() if os.path.exists(HTML) else ''
canon = json.load(io.open(CANON, encoding='utf-8')) if os.path.exists(CANON) else {}
main_rows = collect(html, canon)
main_rows = main_rows + doc_checks(len(html.encode('utf-8')))
main_rows = main_rows + no_old_gen()
main_rows = main_rows + data_checks(html)

# ---------- 负例自检：断言必须能 FAIL，否则治具本身假绿 ----------
garbage = '__DRAW ' + 'x' * 260000
neg1 = collect(garbage, canon)
no_resp = html.replace('@media(max-width:900px)', '').replace('grid-template-columns:1.15fr 1fr', 'GONE')
neg2 = collect(no_resp, canon)
check_self = [
    (failed(neg1) > 0, '负例①占位垃圾必须 FAIL', 'FAIL数=%d' % failed(neg1)),
    (failed(neg2) > 0, '负例②删响应式必须 FAIL', 'FAIL数=%d' % failed(neg2)),
]

w = max([len(n) for _, n, _ in main_rows] + [len(n) for _, n, _ in check_self])
ok = 0
for good, name, detail in main_rows + check_self:
    print(('PASS' if good else 'FAIL') + ' | ' + name.ljust(w) + ' | ' + detail)
    ok += 1 if good else 0
total = len(main_rows) + len(check_self)
print('-' * 60)
print('RESULT %d/%d %s' % (ok, total, 'ALL PASS' if ok == total else 'HAS FAILURES'))
sys.exit(0 if ok == total else 1)

