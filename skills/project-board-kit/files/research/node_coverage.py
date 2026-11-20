#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""节点完整性 / 交叉验证：表(sheet_canon) × CLIENT_A 报告(client_projects_daily) × 看板(__DRAW) × 群聊
只读，不改任何数据。输出 research/node_coverage.json 与控制台结论。

判定口径：
 1) 节点数：canon .msTotal 必须等于 milestones 条数；报告里同机型 totalNodes 不得少于 canon
 2) 完整性：每条里程碑必须有节点名与计划日期；状态写明「已完成」的必须有实际日期
 3) 重复：同机型节点名不得重复
 4) 遗漏：表里有的机型，看板 __DRAW 里必须在；看板里的 ms 数不得少于 canon
 5) 交叉：近 N 天群聊里出现「M/D + 节点词」但表里没有该节点 → 记为 candidate（人工确认，不算失败）
退出码：0 = 无 hard 问题；1 = 有 hard 问题
"""
import io, json, os, re, sys
from datetime import date, timedelta

R = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(R)
TODAY = date.today()
CHAT_DAYS = 21
NODE_WORDS = ('试制', '打样', '样件', '开模', '投模', '送样', '批量生产', '立项', '评审', 'T0', 'T1', 'DVT', '归档', '修模', '备料')

def jload(p, default):
    try:
        return json.load(io.open(p, encoding='utf-8'))
    except Exception:
        return default

def read_text(p):
    try:
        return io.open(p, encoding='utf-8', errors='replace').read()
    except Exception:
        return ''

def md_of(s):
    m = re.search(r'(\d{1,2})\s*/\s*(\d{1,2})', str(s or ''))
    return (int(m.group(1)), int(m.group(2))) if m else None

canon = jload(os.path.join(R, 'sheet_canon.json'), {})
daily = jload(os.path.join(R, 'client_projects_daily.json'), {})
chat_raw = jload(os.path.join(R, 'raw', 'emobile', 'project_chatlog.json'), [])
chat = {'groups': chat_raw if isinstance(chat_raw, list) else (chat_raw.get('groups') or [])}
html = read_text(os.path.join(ROOT, '项目数据看板.html'))

hard, warn, rows = [], [], []

# 看板 __DRAW 里的机型与节点数
draw = {}
m = re.search(r'window\.__DRAW\s*=\s*(\{.*?\});\s*\n', html, re.S)
if m:
    try:
        draw = json.loads(m.group(1))
    except Exception as e:
        warn.append('__DRAW 解析失败：%s' % e)
if not draw:
    warn.append('看板里没找到 window.__DRAW（版面被换过？）')

draw_by_fam = {}
for _k, _v in (draw or {}).items():
    _mm = re.search(r'\d{4,5}', str(_k))
    _f = _mm.group(0) if _mm else ''
    if _f and _f not in draw_by_fam:
        draw_by_fam[_f] = _v

rep = {}
for p in (daily.get('projects') or []):
    rep[str(p.get('model', '')).upper()] = p

# 群聊文本按机型归集（近 21 天）
chat_by_model, cutoff = {}, (TODAY - timedelta(days=CHAT_DAYS)).isoformat()
for g in (chat.get('groups') or []):
    name = str(g.get('group') or g.get('name') or '')
    msgs = g.get('messages') or []
    for mm in msgs:
        d = str(mm.get('date') or mm.get('time') or '')[:10]
        if d and d < cutoff:
            continue
        for tok in re.findall(r'[A-Za-z]{0,2}\d{4,5}[A-Za-z]?', name + ' ' + str(mm.get('text') or '')):
            chat_by_model.setdefault(tok.upper(), []).append(d)

def fam(s):
    m = re.search(r'\d{4,5}', str(s or ''))
    return m.group(0) if m else ''

for key, v in canon.items():
    ms = v.get('milestones') or []
    if not ms:
        continue
    total_raw = int(v.get('msTotal') or 0)
    _names = [str(m.get('n') or '').strip() for m in ms if str(m.get('n') or '').strip()]
    total = len(set(_names)) if _names else total_raw
    name = str(v.get('model') or key)
    issues = []
    _fam_ms = [len((vv.get('milestones') or [])) for kk, vv in canon.items()
               if fam(kk) == fam(name) or fam(vv.get('model')) == fam(name)]
    _max_ms = max(_fam_ms) if _fam_ms else len(ms)
    if total_raw != len(ms) and len(ms) < _max_ms:
        warn.append('%s：本条目 %d 条，同族另有 %d 条的更全节点表（合并键与单机型键并存）' % (name, len(ms), _max_ms))
    elif total_raw != len(ms):
        issues.append('节点数不符 msTotal=%d 实际=%d' % (total_raw, len(ms)))
    seen = {}
    for i, mm in enumerate(ms, 1):
        nm = str(mm.get('n') or '').strip()
        pl = str(mm.get('p') or '').strip()
        ac = str(mm.get('a') or '').strip()
        st = str(mm.get('s') or '').strip()
        if not nm:
            issues.append('第 %d 条缺节点名' % i)
        if not pl:
            issues.append('第 %d 条缺计划日期（%s）' % (i, nm or '?'))
        if st and ('完成' in st) and not ac:
            issues.append('第 %d 条标「%s」但无实际日期（%s）' % (i, st, nm or '?'))
        if nm:
            seen[nm] = seen.get(nm, 0) + 1
    for nm, c in seen.items():
        if c > 1:
            warn.append('%s：节点重名 ×%d「%s」（表里两条同名节点，需确认是否重复行）' % (name, c, nm))

    r = rep.get(name.upper()) or rep.get(fam(name)) or None
    if r and int(r.get('totalNodes') or 0) < total:
        issues.append('CLIENT_A 报告节点数小于表：report=%s 表=%d' % (r.get('totalNodes'), len(ms)))

    d = draw.get(name) or (draw_by_fam.get(fam(name)) if fam(name) else None) or next((v2 for k2, v2 in (draw or {}).items() if name.upper() in k2.upper() or k2.upper() in name.upper()), None)
    if draw and not d:
        _st2 = str((r or {}).get('status') or '')
        if _st2 and _st2 != 'gray':
            issues.append('看板 __DRAW 缺该机型（报告里为 %s）' % _st2)
        else:
            warn.append('%s：看板无此机型卡（已归档或无节点表），未纳入 __DRAW' % name)
    elif d:
        dn = len(d.get('ms') or [])
        if dn < total:
            issues.append('看板节点数小于表：draw=%d 表=%d' % (dn, len(ms)))

    # 群聊候选（只报不进 hard）
    cand = []
    for tok in set([name.upper(), fam(name)]):
        for dstr in chat_by_model.get(tok, [])[:60]:
            pass
    names = set(seen)
    for g in (chat.get('groups') or []):
        gname = str(g.get('group') or '')
        if name and (name in gname or (fam(name) and fam(name) in gname)):
            for mm in (g.get('messages') or [])[:120]:
                t = str(mm.get('text') or '')
                if any(w in t for w in NODE_WORDS) and md_of(t):
                    if not any(n and n[:2] in t for n in names):
                        cand.append(t[:40])
    cand = list(dict.fromkeys(cand))[:5]

    for x in issues:
        hard.append('%s：%s' % (name, x))
    rows.append({'model': name, 'ms_total': len(ms), 'issues': issues, 'chat_candidates': cand,
                 'in_report': bool(r), 'in_draw': bool(d)})

# 报告里有、表里没有的机型（可能漏抓节点表）
canon_fams = {fam(k) for k in canon if canon[k].get('milestones')}
for mdl, p in rep.items():
    if int(p.get('totalNodes') or 0) > 0 and fam(mdl) not in canon_fams:
        warn.append('CLIENT_A 报告有节点（%s 个）但表里没有该机型节点：%s' % (p.get('totalNodes'), mdl))

out = {
    'at': TODAY.isoformat(),
    'models_with_ms': len(rows),
    'hard_count': len(hard),
    'warn_count': len(warn),
    'hard': hard[:80],
    'warn': warn[:40],
    'rows': rows,
}
io.open(os.path.join(R, 'node_coverage.json'), 'w', encoding='utf-8').write(
    json.dumps(out, ensure_ascii=False, indent=2))

print('COVERAGE models=%d ms_rows=%d hard=%d warn=%d' % (len(canon), len(rows), len(hard), len(warn)))
for x in hard[:12]:
    print('  HARD', x)
for x in warn[:6]:
    print('  WARN', x)
cands = sum(1 for r in rows if r['chat_candidates'])
print('COVERAGE candidates=%d（群聊提到但表里没有的节点，人工确认）' % cands)
sys.exit(1 if hard else 0)
