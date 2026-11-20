r"""把腾讯文档页签的文本快照结构化成 sheet_full.json。

输入：research\wxdata\T*_LIVE.txt —— 首行是 "# sheet=… id=… rows=N" 元信息，
其后是 "R003 | 序号 | 里程碑 | …" 形式的行结构化网格（每行一个 R### 行号）。
输出：research\sheet_full.json = {projects, summaries, others, multi}：
  projects  单机型项目页（里程碑 n/s/p/a/st/d）、summaries 概览/归档表、
  multi     通用进度表（机型名向下填充）、others   未能归类的页签。

为什么不走 research\_raw\*.json 的 protobuf：那份导出把表格拆成「列块 +
样式 run」两套坐标，列块与样式 run 的偏移/合并语义没有可靠对应关系，
靠猜坐标还原单元格是死路；文本快照已经把合并单元格按行摊平，反而确定。
"""
import glob, io, json, os, re, sys

# 以 __file__ 反推项目根（research/ 的上一级），避免把绝对路径写死在脚本里。
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WX = os.path.join(ROOT, 'research', 'wxdata')
OUT = os.path.join(ROOT, 'research', 'sheet_full.json')

def parse_live(path):
    # 读单个 T*_LIVE.txt：抽首行 # 元信息，其余只收 "R### |" 开头的网格行。
    rows = []
    meta = {}
    warned = False
    with io.open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            if '\ufffd' in line and not warned:
                print('WARN bad bytes replaced (U+FFFD) in', os.path.basename(path))
                warned = True
            line = line.rstrip('\n')
            if line.startswith('#'):
                m = re.search(r'sheet=(\S+)\s+id=(\S+)\s+rows=(\d+)', line)
                if m:
                    meta = {'sheet': m.group(1), 'id': m.group(2), 'rows': int(m.group(3))}
                continue
            if not re.match(r'^R\d+ \|', line):
                continue
            body = line.split('|', 1)[1]
            cells = [c.strip() for c in body.split('|')]
            while cells and cells[-1] == '':
                cells.pop()
            rows.append(cells)
    return meta, rows

def norm(s):
    return re.sub(r'\s+', '', s or '')

def find_header(rows, key='里程碑'):
    # 只在前 12 行找表头：扫全表会把备注里的「日期/状态」误认成列名。
    for i, r in enumerate(rows[:12]):
        if any(norm(c) == key for c in r):
            return i
    return None

def col_of(header, *names):
    # 按列名定位列号：先精确匹配，再退化为子串包含。
    for n in names:
        for i, c in enumerate(header):
            if norm(c) == n:
                return i
    for n in names:
        for i, c in enumerate(header):
            if n in norm(c):
                return i
    return None

def parse_project(meta, rows):
    # 解析单机型项目页：定位里程碑表头，逐行抽 n/s/p/a/st/d 六个字段。
    hi = find_header(rows, '里程碑')
    if hi is None:
        return None
    header = rows[hi]
    c_ms = col_of(header, '里程碑')
    c_st = col_of(header, '状态')
    c_plan = col_of(header, '现状计划', '当前计划', '计划')
    c_act = col_of(header, '实际完成')
    c_start = col_of(header, '开始时间')
    # d 字段历史名叫 due，实际取「延期原因」列；改名会破坏 sheet_canon 契约。
    c_due = col_of(header, '延期原因')
    ms = []
    for r in rows[hi+1:]:
        if c_ms is None or c_ms >= len(r):
            continue
        name = r[c_ms].strip()
        if not name or name in ('序号',):
            continue
        get = lambda c: (r[c].strip() if c is not None and c < len(r) else '')
        ms.append({
            'n': name,
            's': get(c_st),
            'p': get(c_plan),
            'a': get(c_act),
            'st': get(c_start),
            'd': get(c_due),
        })
    model = meta.get('sheet') or ''
    prod = ''
    for r in rows[:4]:
        if r and '产品类型' in r[0]:
            prod = r[1] if len(r) > 1 else ''
    return {'id': meta.get('id'), 'model': model, 'product': prod,
            'header': header, 'milestones': ms, 'rows': len(rows)}

def parse_summary(meta, rows):
    # 解析概览/归档表：以「机型」为表头，用机型正则滤掉说明行与合计行。
    hi = None
    for i, r in enumerate(rows[:12]):
        if any(norm(c) == '机型' for c in r):
            hi = i; break
    if hi is None:
        return None
    header = rows[hi]
    c_model = col_of(header, '机型')
    c_brand = col_of(header, '品牌')
    c_prio = col_of(header, '优先')
    c_st = col_of(header, '状态')
    c_arch = col_of(header, '归档', '结案')
    out = []
    for r in rows[hi+1:]:
        get = lambda c: (r[c].strip() if c is not None and c < len(r) else '')
        model = get(c_model)
        # 与 parse_multi 保持同一机型正则：两处若各写一套，会同一机型在概览页与节点页被切成两个 key，
        # 下游 sheet_canon 的「取节点最多一份」就再也合并不回来。表头/行元数据来自页签文本快照，不是 XLSX 单元格。
        if not model or not re.search(r'[A-Za-z]{1,2}\d{3,5}|^\d{4}', model):
            continue
        out.append({'model': model, 'brand': get(c_brand), 'prio': get(c_prio),
                    'status': get(c_st), 'archived': get(c_arch)})
    return {'id': meta.get('id'), 'header': header, 'items': out}


def parse_multi(meta, rows):
    r"""解析「通用进度表」：首列机型名只在分组首行出现，后续行留空。
    为什么向下填充：腾讯文档导出的合并单元格在文本快照里只保留矩形左上角
    的值；若不把 cur 带入后续行，同一机型的节点会被归到空 key，最终
    withMilestones 少算。机型识别用 r'[A-Za-z]{1,2}\d{3,5}|^\d{4}'：
    项目机型是 2 字母+4~5 位数字或纯 4 位编号，用窄正则才能把「序号」
    表头、备注、日期串排除掉；'序号' 再显式跳过一次，防止页签里出现
    R001 样式行号被误当新机型。只在前 12 行找表头，是因为该表固定把
    标题/说明放在表头前，扫全表会把备注里的「日期/状态」误认成列名。
    """
    hi = None
    for i, r in enumerate(rows[:12]):
        cells = [norm(c) for c in r]
        if '机型' in cells and any(c == '关键节点' for c in cells):
            hi = i; break
    if hi is None:
        return None
    header = rows[hi]
    c_model = col_of(header, '机型')
    c_ms = col_of(header, '关键节点')
    c_st = col_of(header, '状态')
    c_start = col_of(header, '开始时间')
    c_plan0 = col_of(header, '初始计划')
    c_plan = col_of(header, '现状计划', '计划完成', '计划')
    c_act = col_of(header, '实际完成')
    out = {}
    cur = ''
    for r in rows[hi+1:]:
        get = lambda c: (r[c].strip() if c is not None and c < len(r) else '')
        mm = get(c_model)
        if mm and re.search(r'[A-Za-z]{1,2}\d{3,5}|^\d{4}', mm):
            cur = mm
        name = get(c_ms)
        if not cur or not name or name in ('序号',):
            continue
        out.setdefault(cur, []).append({
            'n': name, 's': get(c_st), 'p': get(c_plan) or get(c_plan0),
            'a': get(c_act), 'st': get(c_start), 'd': '',
        })
    if not out:
        return None
    return {'id': meta.get('id'), 'header': header, 'models': out}

if __name__ == '__main__':
    projects, summaries, others, multis = [], [], [], []
    for p in sorted(glob.glob(os.path.join(WX, 'T*_LIVE.txt'))):
        meta, rows = parse_live(p)
        if not rows:
            continue
        pr = parse_project(meta, rows)
        if pr:
            projects.append(pr); continue
        mu = parse_multi(meta, rows)
        if mu:
            multis.append(mu); continue
        sm = parse_summary(meta, rows)
        if sm and sm['items']:
            summaries.append(sm); continue
        others.append({'file': os.path.basename(p), 'meta': meta, 'rows': len(rows),
                       'first': rows[0][:6] if rows else []})
    data = {'projects': projects, 'summaries': summaries, 'multi': multis, 'others': others}
    io.open(OUT, 'w', encoding='utf-8').write(json.dumps(data, ensure_ascii=False, indent=1))
    print('projects', len(projects), 'summaries', len(summaries), 'multi', len(multis), 'others', len(others))
    for mu in multis:
        print('  MULTI id=%s models=%d %s' % (mu['id'], len(mu['models']), list(mu['models'])[:5]))
    for pr in projects:
        print('  %-12s ms=%-3d prod=%s' % (pr['model'], len(pr['milestones']), pr['product'][:20]))
    for sm in summaries:
        print('  SUMMARY id=%s items=%d header=%s' % (sm['id'], len(sm['items']), sm['header'][:8]))
    for o in others:
        print('  OTHER', o['file'], o['meta'].get('sheet'), o['rows'], o['first'][:4])
