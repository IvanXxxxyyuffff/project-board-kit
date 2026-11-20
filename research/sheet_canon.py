"""sheet_canon.py —— 把 sheet_full.json 归一成「一机型一条」的权威台账 sheet_canon.json。

输入：research/sheet_full.json（sheet_parse.py 产出：sheets[] 汇总表 + multi[] 节点表）
输出：research/sheet_canon.json
      { 机型: {model, brand, prio, status, msTotal, msDone, running, archivedAt,
               milestones:[{n(节点名), s(状态), p(计划 "46303(10/8)"), a(实际), st, d}] } }

关键规则（改动前先读）：
1. brand_group(v)：CLIENT_A / 89 / 019 → CLIENT_A；SELF_CO / SELF_CO → SELF_CO；
   OEM单 / 0单 → OEM单；Pikus 单列；其余 → 其他客户；空 → 未标注。
2. 已结案判定：状态列含「已结案/已归档/结案/已关闭」→ status=已结案，并尽量取归档日期
   （支持 Excel serial 45740→3/24、YYYY-MM-DD、M月D日）。
3. msTotal / msDone 用**去重后的节点名**计数（表里存在同名节点，如 ITEM_X 两条「IQC全检」）。
4. running = 状态不是已结案/已归档。

被谁用：inject_sheet_truth.py（品牌分布/归档浮层/卡片进度/__DRAW）、
        build_daily_brief.mjs 的 CANON_FILL（专项页签补全 12 台机型节点）、
        node_coverage.py（节点完整性交叉验证真值侧）。
改动影响：字段改名会同时打断上面三处；新增字段请同步 docs/00-维护手册.md §3 数据契约。
"""
import io, json, os, re, collections

# 以 __file__ 反推项目根（research/ 的上一级），避免把绝对路径写死在脚本里。
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, 'research', 'sheet_full.json')
OUT = os.path.join(ROOT, 'research', 'sheet_canon.json')

def brand_group(b):
    r"""把原始「品牌」列折叠成看板里的 data-bg 分组名。

    顺序是口径的一部分，不能随意调：
    - 'CLIENT_A'/'89' 先命中：CLIENT_A 是最大存量客户，两张表混用带前导零与不带的
      写法；收窄成 (?!\d) 只锁 89/CLIENT_A 本身，避免把 CLIENT_Ax 之类编号误吞，
      也保证 019 / 0?9\d客户 不被这一支吃掉而仍能各自独立成组。
    - r'0[9]\d客户|1\d\d客户|^1\d\d$' 把编号客户折叠成「其他客户」，
      否则几十个一次性芯片编号会各自成一个分组，筛选栏直接爆掉。
    - SELF_CO 保留为自有品牌，不与代工客户混。
    - 'O单…'（历史 OEM 单渠道标签）统一折叠成 OEM单。
    - 空值显式给「未标注」：65 台归档里有没填品牌的，若落进默认分支会
      漏成以空串为名的浮层分组。
    - 末尾「含'客户'兜底」再收一次漏网编号客户。
    返回值会被 inject_sheet_truth 写成 data-bg；改名等于让已生成 HTML 的
    筛选全部失配，所以分组名本身是契约。
    """
    s = (b or '').strip()
    if s in ('89', 'CLIENT_A') or re.match(r'^0?89(?!\d)', s):
        return 'CLIENT_A'
    if re.search(r'0[9]\d客户|1\d\d客户|^1\d\d$', s):
        return '其他客户'
    if s == 'SELF_CO':
        return 'SELF_CO'
    if s.startswith('O单'):
        return 'OEM单'
    if not s:
        return '未标注'
    return '其他客户' if '客户' in s else s

def serial_to_md(v, today=None):
    """返回 YYYY-MM-DD。Excel 序列号优先；只有 M/D 时按「不晚于今天」推年份。"""
    import datetime as _dt
    if today is None:
        today = _dt.date.today()
    m = re.match(r"^(\d{4,5})\((\d{1,2})/(\d{1,2})\)$", v or "")
    if m:
        serial = int(m.group(1))
        try:
            d = _dt.date(1899, 12, 30) + _dt.timedelta(days=serial)
            return "%04d-%02d-%02d" % (d.year, d.month, d.day)
        except Exception:
            return ""
    m2 = re.match(r"^(\d{1,2})/(\d{1,2})$", v or "")
    if m2:
        mo, dy = int(m2.group(1)), int(m2.group(2))
        y = today.year
        try:
            d = _dt.date(y, mo, dy)
        except Exception:
            return ""
        if d > today:
            y -= 1
        return "%04d-%02d-%02d" % (y, mo, dy)
    return ""

def main():
    d = json.load(io.open(SRC, encoding='utf-8'))
    # 有效概览/归档表至少有 7 行机型数据；不足的是说明块或空合计行，放进来会
    # 把表头/备注当机型，产生伪 model 把 models 数撑大。
    summaries = [s for s in d['summaries'] if len(s['items']) >= 7]
    rows = []
    for s in summaries:
        for it in s['items']:
            rows.append(it)
    # 同机型在归档表与概览表会重复出现：按「有品牌 + 有归档 + 有状态」计分取
    # 最完整的一条。归档表常缺品牌、节点表常缺状态，若后写直接覆盖就会洗掉
    # 先写那份的口径（brand/archived/status 决定分组与归档数）。
    best = {}
    for r in rows:
        k = r['model']
        cur = best.get(k)
        score = bool(r.get('brand')) + bool(r.get('archived')) + bool(r.get('status'))
        if cur is None or score > cur[0]:
            best[k] = (score, r)
    models = {}
    for k, (_, r) in best.items():
        models[k] = {
            'model': k,
            'brand': r.get('brand', ''),
            'brandGroup': brand_group(r.get('brand', '')),
            'prio': r.get('prio', ''),
            'status': r.get('status', ''),
            'archivedAt': serial_to_md(r.get('archived', '')),
            'archivedRaw': r.get('archived', ''),
        }
    # 节点最多一份（项目节点表 + 通用进度表）：同一机型的里程碑常被拆在多个
    # 页签里，按 len 取 max 才保得住完整节点；withMilestones=16 是看板契约，
    # 任一页签少算即破口径。projects / multi 都走这条规则。
    for pr in d.get('projects') or []:
        m = (pr.get('model') or '').strip()
        if not m:
            continue
        cur = models.get(m)
        if cur is None:
            cur = models[m] = {'model': m, 'brand': '', 'brandGroup': '未标注',
                               'prio': '', 'status': '', 'archivedAt': '', 'archivedRaw': ''}
        # 严格 > 而非 >=：相等时保留先写入的非空 milestones，避免后到的空
        # milestones 覆盖已有节点、把 withMilestones 洗少。
        if len(pr.get('milestones') or []) > len(cur.get('milestones') or []):
            cur['milestones'] = pr.get('milestones') or []
            cur['product'] = pr['product']
    # 通用进度表（开模/其它/预立项）：多云表取节点最多的一份
    for mu in d.get('multi') or []:
        for mm, mss in mu['models'].items():
            mm = mm.strip()
            if not mm or not mss:
                continue
            cur = models.get(mm)
            if cur is None:
                cur = models[mm] = {'model': mm, 'brand': '', 'brandGroup': '未标注',
                                    'prio': '', 'status': '', 'archivedAt': '', 'archivedRaw': ''}
            if len(mss) > len(cur.get('milestones') or []):
                cur['milestones'] = mss

    # msDone 只数带实际完成日期 a 的节点（空 a 表示未关单，不能算完成）；
    # running 收「待反馈」——它是在等我方回复、尚未关单，仍算在跑。
    for m in models.values():
        ms = m.get('milestones') or []
        m['msTotal'] = len(ms)
        m['msDone'] = len([x for x in ms if x.get('a')])
        m['running'] = m['status'] in ('进行中', '待反馈')
    # 先写同目录临时文件再 os.replace：看板/简报可能正读 canon.json，
    # 原子替换避免它们读到写了一半的 JSON。newline 走默认（Windows 转 CRLF）保持落盘口径。
    tmp = OUT + '.tmp'
    io.open(tmp, 'w', encoding='utf-8').write(json.dumps(models, ensure_ascii=False, indent=1))
    os.replace(tmp, OUT)

    print('models', len(models))
    print('brand groups', collections.Counter(v['brandGroup'] for v in models.values()).most_common())
    print('status', collections.Counter(v['status'] for v in models.values()).most_common())
    run = [v for v in models.values() if v['running']]
    print('running', len(run), [v['model'] for v in run][:12])
    arch = [v for v in models.values() if v['status'] == '已结案']
    print('archived', len(arch), 'withDate', len([v for v in arch if v['archivedAt']]))
    print('sample archived', [(v['model'], v['brand'], v['archivedAt']) for v in arch[:6]])
    print('with milestones', len([v for v in models.values() if v['msTotal']]))

if __name__ == '__main__':
    main()
