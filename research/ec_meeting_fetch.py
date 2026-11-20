#!/usr/bin/env python3
# -*- coding: utf-8 -*-
r"""ec_meeting_fetch.py —— 抓本人在泛微 Ecology 的会议安排（日/周/月）

原理：
  1) E-Mobile 客户端内嵌浏览器持有 Ecology 会话 Cookie（ecology_JSessionid/JSESSIONID/loginidweaver），
     但客户端不落盘（Cookies 表 0 行），只能从进程内存取。
  2) em_cookie_scan.exe <needle> 扫 E-Mobile 各进程可读内存，打印含 needle 的上下文片段。
  3) 拿到 Cookie 后 POST /api/meeting/calendar/getData
     body {"selectUser":"","selectdate":"YYYY-MM-DD","meetingType":""}
     返回 {"events":[{begindate,enddate,begintime,endtime,name,id,status}], "start","end"}（窗口约 ±3 周）。
  4) 按「今天/本周/本月」切片，写 research/meeting_data.json 供挂件「会议安排」标签渲染。

用法：python ec_meeting_fetch.py [--out <json>] [--date YYYY-MM-DD] [--quiet]
退出码：0 成功；3 取不到 Cookie（客户端未登录）；4 接口返回异常。
"""
BASE_URL = ''  # 运行时由 main() 覆盖；模块级默认值避免 import 即 NameError
import argparse, json, os, re, subprocess, sys, time, urllib.request
from datetime import date, datetime, timedelta

HERE = os.path.dirname(os.path.abspath(__file__))
SCAN = os.path.join(HERE, 'em_cookie_scan.exe')
BASE = ''  # 不做默认值：必须由 config.json 的 emobile.host / ecology.baseUrl 提供
OUT = os.path.join(HERE, 'meeting_data.json')


def load_cfg():
    """解析 Ecology 基地址。顺序：config.json → 环境变量 FRD_EM_HOST → 上次成功记录 → 报错退出。

    新机器第一次用必须先配 host（config.json 的 emobile.host 或 FRD_EM_HOST）；
    本机这种「已经抓过、但没维护 config.json」的场景，沿用数据文件里记录的上次地址，
    避免无谓地在错误的默认值上等 25 秒超时。
    """
    try:
        cfg = json.load(open(os.path.join(HERE, 'config.json'), encoding='utf-8'))
    except Exception:
        cfg = {}
    em = cfg.get('emobile') or {}
    host = em.get('host') or ''
    base = (cfg.get('ecology') or {}).get('baseUrl') or ''
    if base:
        return base.rstrip('/')
    if host:
        return f'http://{host}:8080'
    env = os.environ.get('FRD_EM_HOST') or ''
    if env:
        return env if env.startswith('http') else f'http://{env}:8080'
    try:
        prev = json.load(open(os.path.join(HERE, 'meeting_data.json'), encoding='utf-8'))
        src = str(prev.get('source') or '').rstrip('/')
        if src.startswith('http'):
            return src
    except Exception:
        pass
    print('CONFIG_MISSING emobile.host：先填 research/config.json 的 emobile.host，'
          '或设环境变量 FRD_EM_HOST，或跑 discover.ps1 探测')
    raise SystemExit(2)


def cookie_from_memory():
    """从 E-Mobile 进程内存里捞 Cookie 头。

    旧实现要求值只含 [0-9A-Za-z]，而 ecology_JSessionid 是 base64（含 / + =），
    一旦值里出现这些字符就整条匹配失败，表现为 no_cookie。
    """
    if not os.path.exists(SCAN):
        return None
    try:
        p = subprocess.run([SCAN, 'ecology_JSessionid', '8'], capture_output=True, timeout=240)
    except Exception:
        return None
    txt = p.stdout.decode('utf-8', 'ignore')
    pair_re = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)=([^;\s\x00-\x1f]+)")
    best = None
    for seg in re.split(r"Cookie:\s*", txt)[1:]:
        head = seg.split("\x00")[0][:600]
        pairs = {}
        for k, v in pair_re.findall(head):
            pairs.setdefault(k, v)
        if "ecology_JSessionid" in pairs and ("JSESSIONID" in pairs or len(pairs) >= 2):
            score = len(pairs)
            if best is None or score > best[0]:
                best = (score, pairs)
    if not best:
        return None
    pairs = best[1]
    keep = ["ecology_JSessionid", "JSESSIONID", "loginidweaver", "languageidweaver"]
    parts = [f"{k}={pairs[k]}" for k in keep if k in pairs]
    return "; ".join(parts) if parts else None


def call(cookie, path, body):
    req = urllib.request.Request(BASE_URL + path, data=json.dumps(body).encode('utf-8'),
                                 headers={'Content-Type': 'application/json', 'Cookie': cookie})
    with urllib.request.urlopen(req, timeout=25) as r:
        return json.loads(r.read().decode('utf-8', 'ignore'))


def fetch_window(cookie, day):
    return call(cookie, '/api/meeting/calendar/getData',
                {"selectUser": "", "selectdate": day.isoformat(), "meetingType": ""})


def norm_events(res):
    out = []
    for e in res.get('events') or []:
        b = str(e.get('begindate') or '')[:10]
        out.append({
            'id': str(e.get('id') or ''),
            'date': b,
            'start': str(e.get('begintime') or '')[:5],
            'end': str(e.get('endtime') or '')[:5],
            'name': str(e.get('name') or '').strip(),
            'status': str(e.get('status') or ''),
        })
    return [x for x in out if x['date'] and x['name']]


def slice_for(items, lo, hi):
    rows = [x for x in items if lo.isoformat() <= x['date'] <= hi.isoformat()]
    rows.sort(key=lambda x: (x['date'], x['start'] or '99:99'))
    return rows


def attach_next_detail(cookie, out):
    """用 calendar/getNextMeeting 补齐「下一场会议」的地点与参会人。

    该接口忽略传入的 id，永远返回最近的一场，但提醒场景正好只会用到最近一场。
    """
    try:
        r = call(cookie, '/api/meeting/calendar/getNextMeeting', {"selectdate": out.get("at", "")[:10]})
    except Exception:
        return 0
    name = (r.get('meetingName') or '').strip()
    if not name:
        return 0
    room = (r.get('meetingRoom') or '').strip()
    if not room:
        rooms = r.get('meetingRooms') or []
        room = (rooms[0].get('name') or '').strip() if rooms else ''
    people = '、'.join([str((x or {}).get('name', '')).strip() for x in (r.get('members') or []) if (x or {}).get('name')])
    # toptitle 形如「11月23日(周一)14:30」，必须日期+时间都对上才回填，避免同名会议串场
    import re as _re
    mt = _re.match(r'\s*(\d{1,2})月(\d{1,2})日[^0-9]*(\d{1,2}:\d{2})', (r.get('toptitle') or ''))
    if not mt:
        return 0
    mm_, dd_, tt_ = int(mt.group(1)), int(mt.group(2)), mt.group(3)
    hit = 0
    for sec in ('today', 'week', 'month'):
        for it in (out.get(sec) or {}).get('items') or []:
            dparts = (it.get('date') or '').split('-')
            if len(dparts) != 3:
                continue
            if int(dparts[1]) != mm_ or int(dparts[2]) != dd_ or (it.get('start') or '')[:5] != tt_:
                continue
            if (it.get('name') or '').strip() == name:
                if not it.get('place'):
                    it['place'] = room
                if not it.get('people'):
                    it['people'] = people
                hit += 1
    return hit


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--out', default=OUT)
    ap.add_argument('--date', default=None)
    ap.add_argument('--quiet', action='store_true')
    a = ap.parse_args()
    global BASE_URL
    BASE_URL = load_cfg()
    today = datetime.strptime(a.date, '%Y-%m-%d').date() if a.date else date.today()

    ck = cookie_from_memory()
    if not ck:
        print('MEETING_FAIL reason=no_cookie（请确认 E-Mobile 客户端已登录并开着）')
        return 3

    try:
        r1 = fetch_window(ck, today)
        if not r1.get('api_status'):
            print('MEETING_FAIL reason=api_status body=' + json.dumps(r1)[:200])
            return 4
        r2 = fetch_window(ck, today.replace(day=28))
    except Exception as e:
        print('MEETING_FAIL reason=exception %s' % e)
        return 4

    items = norm_events(r1) + norm_events(r2)
    seen = {}
    for x in items:
        seen[(x['id'], x['date'], x['start'], x['name'])] = x
    items = list(seen.values())

    monday = today - timedelta(days=today.weekday())
    sunday = monday + timedelta(days=6)
    first = today.replace(day=1)
    nxt = (first + timedelta(days=32)).replace(day=1)
    last = nxt - timedelta(days=1)

    payload = {
        'at': datetime.now().strftime('%Y-%m-%d %H:%M'),
        'source': BASE_URL,
        'today': {'label': today.isoformat(), 'items': slice_for(items, today, today)},
        'week': {'label': f'{monday.isoformat()} ~ {sunday.isoformat()}', 'from': monday.isoformat(),
                 'to': sunday.isoformat(), 'items': slice_for(items, monday, sunday)},
        'month': {'label': today.strftime('%Y-%m'), 'from': first.isoformat(), 'to': last.isoformat(),
                  'items': slice_for(items, first, last)},
        'all': sorted(items, key=lambda x: (x['date'], x['start'] or '99:99')),
        'window': {'start': r1.get('start'), 'end': r1.get('end')},
    }
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    attach_next_detail(ck, payload)
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(payload, f, ensure_ascii=False, indent=1)
    if not a.quiet:
        print('MEETING OK today=%d week=%d month=%d all=%d cookie_len=%d'
              % (len(payload['today']['items']), len(payload['week']['items']),
                 len(payload['month']['items']), len(payload['all']), len(ck)))
    return 0


if __name__ == '__main__':
    sys.exit(main())

MEM_CACHE = {}


def _strip_td(html):
    """从 Weaver tableString 里抽单元格文本，按出现顺序返回。"""
    txt = re.sub(r"<[^>]+>", "\n", html or "")
    out = []
    for line in txt.split("\n"):
        v = line.strip()
        if v and not v.startswith("&"):
            out.append(v)
    return out


def member_names(cookie, mid):
    """参会人员：/api/meeting/member/getMemberList 的 hrmdatas.tableString。"""
    if mid in MEM_CACHE:
        return MEM_CACHE[mid]
    names = []
    try:
        r = call(cookie, '/api/meeting/member/getMemberList', {"id": str(mid)})
        ts = ((r.get('hrmdatas') or {}).get('tableString') or '')
        cells = _strip_td(ts)
        # 表头 13 列，之后每行取第一列（membername）
        body = cells[13:] if len(cells) > 13 else cells
        for i, v in enumerate(body):
            if i % 13 == 0 and 1 < len(v) <= 12:
                names.append(v)
    except Exception:
        pass
    MEM_CACHE[mid] = names
    return names


def meeting_place(cookie, mid):
    """地点：先从会议详情接口试，取不到就返回空串。"""
    for path, body in (
        ('/api/meeting/report/getDetail', {"id": str(mid)}),
        ('/api/meeting/base/getMeetingTabElement', {"id": str(mid)}),
    ):
        try:
            r = call(cookie, path, body)
        except Exception:
            continue
        txt = json.dumps(r, ensure_ascii=False)
        m = re.search(r'"(?:location|address|meetingroom|roomname)"\s*:\s*"([^"]{1,40})"', txt)
        if m:
            return m.group(1)
    return ""
