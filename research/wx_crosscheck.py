"""三源交叉验证：企微文档(节点表) × E-Mobile 群聊 × 企微本地群聊。

用途
  1) 同一项目在三个数据源上的状态是否一致；
  2) E-Mobile 抓到的群聊与企微本机库抓到的群聊，互相验证（重合/独有）；
  3) 给出项目卡点判定：一致-风险 / 冲突(文档说完成但群里还在改) / 风险·群聊静默。

输入
  research/sheet_canon.json                     文档侧：机型→milestones/msTotal/msDone/status/brand
  research/raw/emobile/project_chatlog.json     E-Mobile 侧：[{group,messages:[{time,ts,sender,text}]}]
  research/wxwork_chat.json                     企微本地侧：{conversations:[{name,kind,msgs:[{t,ts,from,text}]}]}

输出
  research/wx_crosscheck.json  {at, days, summary:{...}, projects:[...]}

用法
  python wx_crosscheck.py [--days 14] [--out research/wx_crosscheck.json]
"""
import argparse
import datetime as dt
import json
import os
import re
import sys

RISK = re.compile(r'延期|延后|卡住|卡在|卡了|缺料|欠料|返修|重做|异常|不良|报废|待确认|等客户|等回复|来不及|赶不上|做不出|对不上|错位|变形|质量异常|结构异常|超差|跳票|风险')
GOOD = re.compile(r'已确认|确认OK|确认可以|完成|通过|没问题|正常|已解决|已到位|OK\b|可以\b', re.I)
NUM = re.compile(r'(\d{3,5})[A-Z]?')


def fam(txt):
    """机型族：取 3–5 位数字，统一成 5 相位数字串，用于宽松匹配。"""
    out = set()
    for m in NUM.finditer(str(txt or '')):
        d = m.group(1)
        out.add(d)
        if len(d) >= 4:
            out.add(d[-4:])
    return out


def load_json(p, default=None):
    if not os.path.exists(p):
        return default
    with open(p, encoding='utf-8') as f:
        return json.load(f)


def ts_of(s):
    try:
        v = int(str(s)[:13])
    except Exception:
        return 0
    return v // 1000 if v > 1e12 else v


def parse_dt(s):
    for fmt in ('%Y-%m-%d %H:%M', '%Y/%m/%d %H:%M', '%Y-%m-%d'):
        try:
            return dt.datetime.strptime(s, fmt)
        except Exception:
            continue
    return None


def canon_side(canon):
    rows = {}
    for key, c in (canon or {}).items():
        if not isinstance(c, dict):
            continue
        ms = c.get('milestones') or []
        total = c.get('msTotal') or len(ms)
        done = c.get('msDone')
        if done is None:
            done = sum(1 for m in ms if (m.get('a') or '').strip())
        today = dt.date.today()
        overdue = []
        for m in ms:
            p = str(m.get('p') or '')
            a = str(m.get('a') or '').strip()
            mm = re.search(r'\((\d{1,2})/(\d{1,2})\)', p)
            if a or not mm:
                continue
            try:
                d = dt.date(today.year, int(mm.group(1)), int(mm.group(2)))
            except Exception:
                continue
            if d.year < today.year - 1:
                d = dt.date(today.year + 1, d.month, d.day)
            if d < today:
                overdue.append((m.get('n') or '', d.strftime('%m-%d')))
        rows[key] = {
            'model': c.get('model') or key,
            'status': c.get('status') or '',
            'brand': c.get('brandGroup') or '',
            'ms_total': total,
            'ms_done': done,
            'overdue': overdue,
            'fams': fam(key) | fam(c.get('model')),
        }
    return rows


def chat_side(em, wx, days):
    cutoff = (dt.datetime.now() - dt.timedelta(days=days)).timestamp()
    em_rows, wx_rows = [], []
    for g in (em or []):
        msgs = [m for m in (g.get('messages') or []) if ts_of(m.get('ts') or parse_dt(m.get('time') or '') and 0) >= cutoff
                or (parse_dt(m.get('time') or '') or dt.datetime.min).timestamp() >= cutoff]
        em_rows.append({'name': g.get('group') or g.get('name') or '', 'msgs': msgs,
                        'fams': fam(g.get('group'))})
    for c in (wx or {}).get('conversations', []):
        if c.get('kind') != 'group':
            continue
        msgs = [m for m in (c.get('msgs') or []) if (m.get('ts') or 0) >= cutoff]
        fams = fam(c.get('name'))
        for m in msgs:
            fams |= fam(m.get('text'))
        wx_rows.append({'name': c.get('name') or '', 'msgs': msgs, 'fams': fams})
    return em_rows, wx_rows


def pick(rows, fams):
    """按机型族挑最相关的会话：族匹配优先，其次名字含数字。"""
    best = None
    for r in rows:
        hit = len(r['fams'] & fams)
        score = hit * 1000 + len(r['msgs'])
        if hit and (best is None or score > best[0]):
            best = (score, r)
    return best[1] if best else None


def stats(msgs, key_text):
    risk = [m for m in msgs if RISK.search(key_text(m))]
    good = [m for m in msgs if GOOD.search(key_text(m))]
    last = max([ts_of(m.get('ts') or 0) for m in msgs], default=0)
    return risk, good, last


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--days', type=int, default=14)
    ap.add_argument('--canon', default='research/sheet_canon.json')
    ap.add_argument('--em', default='research/raw/emobile/project_chatlog.json')
    ap.add_argument('--wx', default='research/wxwork_chat.json')
    ap.add_argument('--out', default='research/wx_crosscheck.json')
    a = ap.parse_args()

    canon = load_json(a.canon, {})
    em = load_json(a.em, [])
    wx = load_json(a.wx, {})
    if not canon:
        print('MISSING canon', a.canon)
        return 2

    doc = canon_side(canon)
    em_rows, wx_rows = chat_side(em, wx, a.days)

    projects = []
    for key, d in doc.items():
        if not d['ms_total']:
            continue
        e = pick(em_rows, d['fams'])
        w = pick(wx_rows, d['fams'])
        e_msgs = e['msgs'] if e else []
        w_msgs = w['msgs'] if w else []
        ek = lambda m: str(m.get('text') or '')
        wk = lambda m: str(m.get('text') or '')
        e_risk, e_good, e_last = stats(e_msgs, ek)
        w_risk, w_good, w_last = stats(w_msgs, wk)

        # 两源互相验证：按去空白文本比对重合
        e_set = {re.sub(r'\s+', '', ek(m)) for m in e_msgs if ek(m)}
        w_set = {re.sub(r'\s+', '', wk(m)) for m in w_msgs if wk(m)}
        both = len(e_set & w_set)
        em_only = len(e_set - w_set)
        wx_only = len(w_set - e_set)

        verdict, why = '一致-正常', ''
        overdue = d['overdue']
        chat_risk = e_risk + w_risk
        if overdue and chat_risk:
            verdict, why = '一致-风险', '节点逾期且群聊有风险信号'
        elif overdue and not chat_risk:
            verdict, why = '风险·群聊静默', '节点逾期，两源近 %d 天无风险消息' % a.days
        elif d['ms_done'] >= d['ms_total'] and chat_risk:
            verdict, why = '冲突-文档已完成/群聊仍异常', '节点表全完成，但群聊近 %d 天有风险词' % a.days
        elif not (e or w):
            verdict, why = '单一来源', '两源均未匹配到该机型会话'
        elif d['ms_done'] < d['ms_total'] and not chat_risk:
            verdict, why = '一致-推进中', '节点未完但群聊无风险信号'

        projects.append({
            'model': d['model'],
            'status': d['status'],
            'brand': d['brand'],
            'doc': {'total': d['ms_total'], 'done': d['ms_done'],
                    'overdue': overdue[:6], 'overdue_n': len(overdue)},
            'em': {'conv': e['name'] if e else '', 'msgs': len(e_msgs), 'risk': len(e_risk),
                   'good': len(e_good), 'last': dt.datetime.fromtimestamp(e_last).strftime('%m-%d %H:%M') if e_last else '',
                   'risk_samples': [ek(m)[:60] for m in e_risk[:3]]},
            'wx': {'conv': w['name'] if w else '', 'msgs': len(w_msgs), 'risk': len(w_risk),
                   'good': len(w_good), 'last': dt.datetime.fromtimestamp(w_last).strftime('%m-%d %H:%M') if w_last else '',
                   'risk_samples': [wk(m)[:60] for m in w_risk[:3]]},
            'verify': {'both': both, 'em_only': em_only, 'wx_only': wx_only,
                       'agree_rate': round(both / max(1, both + em_only + wx_only), 3)},
            'verdict': verdict,
            'why': why,
        })

    projects.sort(key=lambda p: (-p['doc']['overdue_n'], -(p['em']['risk'] + p['wx']['risk'])))
    cnt = {}
    for p in projects:
        cnt[p['verdict']] = cnt.get(p['verdict'], 0) + 1
    summary = {
        'projects': len(projects),
        'groups_em': len(em_rows),
        'groups_wx': len(wx_rows),
        'verdicts': cnt,
        'overlap_msgs': sum(p['verify']['both'] for p in projects),
        'em_only_msgs': sum(p['verify']['em_only'] for p in projects),
        'wx_only_msgs': sum(p['verify']['wx_only'] for p in projects),
    }
    payload = {'at': dt.datetime.now().strftime('%Y-%m-%d %H:%M'), 'days': a.days,
               'summary': summary, 'projects': projects}
    os.makedirs(os.path.dirname(os.path.abspath(a.out)) or '.', exist_ok=True)
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(payload, f, ensure_ascii=False, indent=1)

    print('XC OK projects=%d em_groups=%d wx_groups=%d overlap=%d em_only=%d wx_only=%d'
          % (summary['projects'], summary['groups_em'], summary['groups_wx'],
             summary['overlap_msgs'], summary['em_only_msgs'], summary['wx_only_msgs']))
    for k, v in cnt.items():
        print('   %-22s %d' % (k, v))
    for p in projects[:6]:
        print('  %-14s doc=%d/%d 逾期%-2d em=%d条/风险%d wx=%d条/风险%d 验证(共%d独%d/%d) %s'
              % (p['model'][:14], p['doc']['done'], p['doc']['total'], p['doc']['overdue_n'],
                 p['em']['msgs'], p['em']['risk'], p['wx']['msgs'], p['wx']['risk'],
                 p['verify']['both'], p['verify']['em_only'], p['verify']['wx_only'], p['verdict']))
    return 0


if __name__ == '__main__':
    sys.exit(main())
