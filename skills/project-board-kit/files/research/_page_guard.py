"""_page_guard.py —— 新生成页的静态自检（接在管道 5a 之后，失败不重建数据）。

检查项：
1. title 含今天（否则页面是隔夜快照）
2. 体积 > 150000 字节
3. 6 处结构存在：sec-nodes / daybox / kpi-pop-css / ring-svg-fix / st-modal / id="fs"
4. 14 根 .hcol 热力柱
5. 柱顶数字之和 == 面板副标「N 条」（口径：条 = 明细行数）
6. 明细行模板 dayrow + dup-tag、图例「同族重复」
7. 数据完整性.json 的 at 日期 == 今天（硬失败但不触发重建，因为重建改不了数据）

自愈：结构类失败 → 用 research/_模板_v4.html 重置 + 跑 _v4_rebuild.py --inplace + _kpi_pop.py 再复检。
输出：research/page_guard.json；退出码 1 = 有 hard。
"""
import hashlib, io, json, os, re, shutil, subprocess, sys, time

ROOT = os.environ.get('PROJECT_ROOT') or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RS = os.path.join(ROOT, 'research')
PAGE = os.path.join(ROOT, '项目数据看板.html')
TMPL = os.path.join(RS, '_模板_v4.html')
OUTJ = os.path.join(RS, 'page_guard.json')
PY = sys.executable

# 治具清单（name, 所在目录）：体检时扫占位符 + 与套件副本比哈希
KIT_RESEARCH = os.path.join(ROOT, 'skills', 'project-board-kit', 'files', 'research')
HARNESS = [('verify_board.py', RS), ('verify_runtime.mjs', RS), ('_page_guard.py', RS),
           ('node_coverage.py', RS), ('client_progress_report.ps1', RS),
           ('client_confirm_send.ps1', RS), ('每日项目简报.ps1', ROOT)]


def today():
    return time.strftime('%Y-%m-%d')


def read(p):
    return io.open(p, encoding='utf-8', errors='replace').read() if os.path.exists(p) else ''


def check():
    """返回 (hard_fails, soft_warns, info)"""
    hard, soft, info = [], [], {}
    if not os.path.exists(PAGE):
        return ['页面不存在'], soft, info
    h = read(PAGE)
    info['bytes'] = len(h.encode('utf-8'))
    m = re.search(r'<title>(.*?)</title>', h, re.S)
    title = (m.group(1).strip() if m else '')
    info['title'] = title
    if today() not in title:
        hard.append('title 不含今天 %s（实际 %s）' % (today(), title))
    if info['bytes'] < 150000:
        hard.append('页面体积异常 %d B' % info['bytes'])
    for tok, name in [('id="sec-nodes"', '04 时间轴'), ('id="daybox"', '当日明细'),
                      ('kpi-pop-css', 'KPI 弹层样式'), ('ring-svg-fix', '圆环脚本'),
                      ('st-modal', '归档浮层'), ('id="fs"', '筛选栏')]:
        if tok not in h:
            hard.append('缺结构：%s' % name)
    cols = re.findall(r'class="hcol[^"]*"[^>]*data-day="(\d{4}-\d{2}-\d{2})"', h)
    info['cols'] = len(cols)
    if len(cols) != 14:
        hard.append('热力柱应为 14 列，实际 %d' % len(cols))

    # 柱顶数字之和 vs 面板副标「N 条」（条=明细行，口径唯一）
    sub = re.search(r'未来 14 天节点密度</b><span>(\d+) 条 · 覆盖 (\d+) 台机型', h)
    tot = 0
    for mm in re.finditer(r'<span class="hbar"[^>]*>(?:<b>(\d{1,3})</b>)?', h):
        tot += int(mm.group(1) or 0)
    info['col_sum'] = tot
    if sub:
        info['sub_rows'] = int(sub.group(1))
        info['sub_models'] = int(sub.group(2))
        if tot != info['sub_rows']:
            hard.append('柱数字之和 %d ≠ 面板副标 %d 条' % (tot, info['sub_rows']))
    else:
        hard.append('面板副标缺「N 条 · 覆盖 M 台机型」')
    # 同一页面的「未来 14 天」数字必须一致（历史上 KPI 30 / 标题 29 并存过两轮）
    m_sec = re.search(r'未来 14 天节点\s*(\d+)', h)
    m_kpi = re.search(r'data-kpi="nodes".{0,400}?data-n="(\d+)"', h, re.S)
    if not (m_sec and m_kpi):
        hard.append('未来节点口径断言无法取样：标题匹配=%s KPI 匹配=%s' % (bool(m_sec), bool(m_kpi)))
    elif m_sec.group(1) != m_kpi.group(1):
        hard.append('未来节点口径不一致：标题 %s 个 vs KPI %s 个' % (m_sec.group(1), m_kpi.group(1)))

    # 明细头模板必须按「条」计数
    if 'list.length' not in h:
        hard.append('明细头缺「list.length 条」计数')
    if 'dayrow' not in h or 'dup-tag' not in h:
        hard.append('明细行缺同族机型模板（dayrow/dup-tag）')
    if '同族重复' not in h:
        hard.append('甘特图例缺「同族重复（空心）」')

    # 群聊最新时间（软告警：网络未通时不判页面坏）
    cl = os.path.join(RS, 'raw', 'emobile', 'project_chatlog.json')
    try:
        d = json.loads(read(cl) or '[]')
        groups = d if isinstance(d, list) else (d.get('groups') or [])
        newest = ''
        for g in groups:
            for msg in (g.get('messages') or []) if isinstance(g, dict) else []:
                t = str(msg.get('time') or '')
                newest = max(newest, t)
        info['chat_newest'] = newest
        if newest[:10] != today():
            soft.append('群聊最新 %s（非今天）' % (newest or '—'))
    except Exception as e:
        soft.append('chatlog 读取失败：%s' % e)

    # 数据新鲜度（硬失败但不重建页面：重建也改不了数据，需人看 cookie / 客户端）
    for name, key in [('数据完整性.json', 'at')]:
        p = os.path.join(RS, name)
        try:
            j = json.loads(read(p) or '{}')
            at = str(j.get(key) or '')
            info['data_at'] = at
            if at[:10] != today():
                hard.append('数据未更新（%s %s = %s）' % (name, key, at or '—'))
        except Exception as e:
            soft.append('%s 读取失败：%s' % (name, e))
    # 生成页新鲜度
    gen = os.path.join(ROOT, '最新项目简报.html')
    if os.path.exists(gen):
        info['gen_age_min'] = round((time.time() - os.path.getmtime(gen)) / 60, 1)
        if info['gen_age_min'] > 720:
            soft.append('生成页已 %.0f 分钟未更新' % info['gen_age_min'])
    else:
        soft.append('生成页不存在')
    # 治具自检（2026-11-18 审计）：防「占位符残留」「套件副本漂移」再把体检搞成假红
    hh, hn = harness_scan()
    info['harness'] = hh
    soft.extend(hn)
    return hard, soft, info


def harness_scan():
    """只报不改：治具文件里的占位符残留 + 与套件副本的哈希漂移。

    背景：2026-11-17 verify_runtime.mjs 的默认地址被批量替换成
    'http://127.0.0.1:<PORT>/board.html'，导致每次体检都 Invalid navigation URL；
    另一路是 skills/project-board-kit/files 里的旧副本，改好一台不会自动跟着改。
    所以每次体检都顺手扫一遍，命中只进 soft（不能进 hard —— hard 会触发重建页面）。
    """
    hits, notes, drift = [], [], 0
    pats = [re.compile(p) for p in (r'127\.0\.0\.1:<', r'<PORT>', r'<acct>',
                                    r'github\.com/<', r'<TODO>', r'<你的')]
    for name, base in HARNESS:
        p = os.path.join(base, name)
        if not os.path.exists(p):
            notes.append('治具缺失 %s' % name)
            continue
        txt = read(p)
        # 本文件自带这些模式串（是规则本体），跳过，避免自指误报
        for rx in ([] if name == '_page_guard.py' else pats):
            m = rx.search(txt)
            if m:
                seg = txt[max(0, m.start() - 24):m.end() + 12].replace('\n', ' ')
                hits.append('占位符残留 %s :: …%s…' % (name, seg))
        kit = os.path.join(KIT_RESEARCH, name)
        # client_* 是客户专属脚本：套件里那份是脱敏后的通用版，**有意分叉**，不比哈希
        if os.path.exists(kit) and not name.startswith('client_'):
            if hashlib.sha256(open(p, 'rb').read()).hexdigest() != \
               hashlib.sha256(open(kit, 'rb').read()).hexdigest():
                drift += 1
    if drift:
        notes.append('治具与套件副本漂移 %d 个（跑 research/_kit_sync.py 对齐）' % drift)
    if hits:
        notes.extend(hits)
    return hits, notes


def rebuild():
    log = []
    if os.path.exists(PAGE):
        # 自愈会用模板重置现役页：先留一份，出事能回滚（模板不含新功能）
        try:
            bakdir = os.path.join(os.path.dirname(PAGE), 'research', '_bak')
            os.makedirs(bakdir, exist_ok=True)
            bak = os.path.join(bakdir, os.path.basename(PAGE) + '.bak-' + time.strftime('%Y%m%d-%H%M%S'))
            shutil.copy2(PAGE, bak)
            log.append('已备份 ' + os.path.basename(bak))
        except OSError as e:
            log.append('备份失败 ' + str(e))
    if os.path.exists(TMPL):
        io.open(PAGE, 'w', encoding='utf-8', newline='').write(read(TMPL))
        log.append('已用模板重置页面')
    for s in ['_v4_rebuild.py', '_kpi_pop.py']:
        p = os.path.join(RS, s)
        if not os.path.exists(p):
            log.append('缺脚本 %s' % s)
            continue
        a = [PY, '-X', 'utf8', p] + (['--inplace'] if s == '_v4_rebuild.py' else [])
        r = subprocess.run(a, capture_output=True, text=True, encoding='utf-8', errors='replace')
        log.append('%s exit=%d' % (s, r.returncode))
    return log


def main():
    hard, soft, info = check()
    act = []
    struct = [x for x in hard if not x.startswith('数据未更新')]
    if struct:
        act = rebuild()
        hard2, soft2, info2 = check()
        info.update({'after_rebuild': info2})
        res = {'at': time.strftime('%Y-%m-%d %H:%M:%S'), 'stage': 'rebuilt',
               'hard_before': hard, 'hard_after': hard2, 'soft': soft + soft2,
               'actions': act, 'info': info}
        io.open(OUTJ, 'w', encoding='utf-8').write(json.dumps(res, ensure_ascii=False, indent=1))
        if hard2:
            print('GUARD FAIL ' + ' | '.join(hard2))
            return 1
        print('GUARD RECOVERED ' + ' | '.join(hard))
        return 0
    res = {'at': time.strftime('%Y-%m-%d %H:%M:%S'), 'stage': 'ok',
           'hard': [], 'soft': soft, 'info': info}
    io.open(OUTJ, 'w', encoding='utf-8').write(json.dumps(res, ensure_ascii=False, indent=1))
    print('GUARD OK bytes=%d cols=%d nodes=%s soft=%d' % (info['bytes'], info['cols'],
          info.get('sub_rows'), len(soft)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
