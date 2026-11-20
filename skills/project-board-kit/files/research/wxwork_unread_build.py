"""企微本地库 → 未读信号（供 em_unread_live.js 合并进挂件未读区块）。

做三件事：
  1) 从客户端 Data 目录复制 session.db / message.db / user.db 到 _wxdec（客户端独占，必须先复制）；
  2) 用 research/_wxdec/_key.txt 里的 16 字节主密钥离线解密（wxwork_db_decrypt.py 的算法）；
  3) 调 wxwork_chat_export.py 生成 research/wxwork_chat.json。

再输出一份精简的未读行（同 em_unread_live.js 的 signals 形状）：
  {"t":"HH:MM","sender":"某某·企微" 或 "某某·企微私聊","text":"…","model":"ITEM_X","ts":…}

用法：
  python wxwork_unread_build.py                 # 重建 wxwork_chat.json 并打印未读行数
  python wxwork_unread_build.py --hours 2 --json research/wxwork_unread.json
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import datetime as dt

def _tlabel(ts):
    """今天只显示 HH:MM；非今天带 MM-DD，避免把旧消息误读成今天的。"""
    d = dt.datetime.fromtimestamp(ts)
    if d.date() == dt.date.today():
        return d.strftime('%H:%M')
    return d.strftime('%m-%d %H:%M')



HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
try:
    import wxwork_db_decrypt as wdec  # noqa: E402
except Exception:  # 公开套件不含解密器（属内部树）：整条未读通道降级，不让 import 直接崩
    wdec = None

def _find_acc():
    """企微账号 Data 目录（换机/换账号不用改代码）。

    优先级：WXWORK_DATA 环境变量 → ~/Documents/WXWork 下带 Data 的账号目录
    （多个时取 message.db 最新的那个）→ 退回旧默认。
    """
    env = os.environ.get('WXWORK_DATA')
    if env and os.path.isdir(env):
        return env
    base = os.path.join(os.path.expanduser('~'), 'Documents', 'WXWork')
    cands = []
    if os.path.isdir(base):
        for name in os.listdir(base):
            p = os.path.join(base, name, 'Data')
            if os.path.isdir(p):
                mt = 0.0
                for f in ('message.db', 'session.db', 'user.db'):
                    fp = os.path.join(p, f)
                    if os.path.exists(fp):
                        mt = max(mt, os.path.getmtime(fp))
                cands.append((mt, p))
    if cands:
        return sorted(cands)[-1][1]
    return os.path.join(base, 'UNKNOWN', 'Data')


ACC = _find_acc()
DEC = os.path.join(HERE, '_wxdec')
KEYF = os.path.join(DEC, '_key.txt')
MODEL_RE = re.compile(r'(?<![0-9A-Za-z])(?:[A-Za-z]{2})?\d{4,6}[A-Za-z]?(?![0-9A-Za-z])')
# 文件名/正文里的完整日期（2026.11.19、2026-11-19、2026年11月19日）不算型号
DATE_RE = re.compile(r'\d{4}\s*[.\-／/年]\s*\d{1,2}\s*[.\-／/月]\s*\d{1,2}\s*日?')


_WL = {}


def _model_whitelist():
    """项目表里的真实型号集合（sheet_canon.json）。空集合表示拿不到，退回纯正则。"""
    if 'v' in _WL:
        return _WL['v']
    out = set()
    try:
        d = json.load(open(os.path.join(HERE, 'sheet_canon.json'), encoding='utf-8'))
        for k, v in d.items():
            out.add(str(k).strip())
            m = (v or {}).get('model') if isinstance(v, dict) else None
            if m:
                out.add(str(m).strip())
    except Exception:
        out = set()
    _WL['v'] = out
    return out


def _model_of(*parts):
    """挑一个项目型号：先抹日期、排除纯年份，再要求命中项目表；项目表拿不到才退回正则。"""
    s = DATE_RE.sub(' ', ' '.join([p for p in parts if p]))
    wl = _model_whitelist()
    for tok in MODEL_RE.findall(s):
        if len(tok) == 4 and tok.isdigit() and 1900 <= int(tok) <= 2099:
            continue
        if not wl or tok in wl:
            return tok
    if wl:
        for tok in wl:
            if len(tok) >= 4 and tok in s:
                return tok
        return ''
    return ''


def read_key():
    if not os.path.exists(KEYF):
        return None
    k = open(KEYF, 'r', encoding='ascii').read().strip()
    return bytes.fromhex(k) if re.fullmatch(r'[0-9a-fA-F]{32}', k or '') else None


def refresh_plain(key):
    """复制 → 解密到 _wxdec/*_plain.db。返回 True/False。"""
    if wdec is None:
        print('WXU reason=no-decryptor（本套件不含 wxwork_db_decrypt.py，未读通道停用）')
        return False
    ok = True
    for db, out in (('session.db', 'session_plain.db'), ('message.db', 'msg_plain.db'), ('user.db', 'user_plain.db')):
        src = os.path.join(ACC, db)
        if not os.path.exists(src):
            ok = False
            continue
        os.makedirs(DEC, exist_ok=True)
        tmp = os.path.join(tempfile.gettempdir(), 'wxcp_' + db)
        try:
            shutil.copyfile(src, tmp)
        except Exception:
            ok = False
            continue
        dst_dir = os.path.join(DEC, out)
        os.makedirs(dst_dir, exist_ok=True)
        dst = os.path.join(dst_dir, db)
        try:
            pages = wdec.decrypt_db(tmp, dst, key)
            if not pages:
                ok = False
        except Exception:
            ok = False
        try:
            os.remove(tmp)
        except Exception:
            pass
    return ok


def build_chat():
    py = sys.executable
    cmd = [py, '-X', 'utf8', os.path.join(HERE, 'wxwork_chat_export.py'),
           '--dec', DEC, '--out', os.path.join(HERE, 'wxwork_chat.json'), '--days', '14']
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    return (p.returncode == 0), (p.stdout or p.stderr or '').strip().splitlines()[-1:] 


SYS_RE = re.compile(r'删除了一条评论|撤回了一条消息|加入了群聊|移出了群聊|修改群名为|拍了拍|开启了群聊|领取了|邀请.*加入')
# 系统/机器人会话：不算「未读消息」
BOT_CONV = {'文件传输助手', '行业资讯', '微盘', '打卡', '公告', '日历', '日程', '审批', '会议',
            '企业支付', '企业微信团队', '微信运动', '群公告', '通知', '系统消息'}
BOT_RE = re.compile(r'(助手|小秘书|机器人|通知|公告|资讯|打卡|微盘|日程|审批|会议)$')
BOT_TEXT = re.compile(r'自动化流程|已更新，前往查看|系统消息|您有新的|点击查看|工单|审批单')


def _clean_text(t):
    """只保留中文/ASCII/常用标点，去掉 @ 标记里的私有区噪声。"""
    out = []
    for ch in t or '':
        o = ord(ch)
        if 0x4E00 <= o <= 0x9FFF or 0x3000 <= o <= 0x303F or 0xFF00 <= o <= 0xFF65:
            out.append(ch)
        elif 0x20 <= o <= 0x7E:
            out.append(ch)
    return ' '.join(''.join(out).split())


def _clip(s, n):
    """按长度截断但保住扩展名：文件名尾巴（.pptx (137 MB)）不能被切成乱码。"""
    if len(s) <= n:
        return s
    m = re.search(r'\.[A-Za-z0-9]{1,6}\b', s)
    ext = m.group(0) if m else ''
    keep = n - len(ext) - 1
    if keep < 8:
        return s[:n - 1] + '…'
    return s[:keep] + '…' + ext


def _type_label(t):
    """非文本消息的类型占位。映射按 message.db 实测（2026-11-19），不再猜。"""
    m = {
        14: '[图片]', 101: '[图片]', 123: '[图片]',
        15: '[文件]', 20: '[文件]', 34: '[文件]', 102: '[文件]',
        22: '[视频]', 23: '[视频]', 103: '[视频]',
        4: '[名片]', 28: '[邮件提醒]', 31: '[资讯]', 38: '[通知]', 40: '[通话]',
        561: '[打卡]', 565: '[微盘文件]', 573: '[提醒]', 675: '[待办]', 1022: '[系统]',
    }
    try:
        n = int(t)
    except (TypeError, ValueError):
        n = 0
    return m.get(n, '[非文本消息]')


def unread_rows(hours=24, limit=4):
    """近 N 小时里别人发的**文本**消息，按会话取最新一条。

    **回退路径**（未读表不可用时）：按时间窗取最近消息；主路径见 `_unread_counts()`。
    无名会话（unknown）也不进未读，避免出现「某人·企微私聊 删除了一条评论」这种噪音。
    """
    p = os.path.join(HERE, 'wxwork_chat.json')
    if not os.path.exists(p):
        # 没有 chat.json 不等于「没有未读」：返回 None 让调用方回退到时间窗，否则面板会静默空态
        print('WXU reason=no-chat（chat.json 缺失，回退时间窗）')
        return None
    d = json.load(open(p, encoding='utf-8'))
    cutoff = (dt.datetime.now() - dt.timedelta(hours=hours)).timestamp()
    rows = []
    for c in d.get('conversations', []):
        if c.get('kind') not in ('group', 'direct'):
            continue
        msgs = [m for m in (c.get('msgs') or [])
                if (m.get('ts') or 0) >= cutoff and m.get('type') == 2]
        if not msgs:
            continue
        msgs.sort(key=lambda m: m['ts'])
        last = msgs[-1]
        text = _clean_text((last.get('text') or '').strip())
        if len(text) < 2 or SYS_RE.search(text) or BOT_TEXT.search(text):
            continue
        name = (c.get('name') or '').strip()
        if c.get('kind') == 'direct' and (name in BOT_CONV or BOT_RE.search(name)):
            continue
        fa = (last.get('from') or '').strip()
        models = [m for m in [_model_of(text, name)] if m]
        rows.append({
            'ts': last['ts'],
            't': _tlabel(last['ts']),
            'sender': (fa or name)[:10],
            'text': text if len(text) <= 52 else text[:52] + '…',
            'model': models[0] if models else '',
            'conv': name,
            'src': '企微',
            'kind': c.get('kind'),
        })
    rows.sort(key=lambda r: -r['ts'])
    return rows[:limit]




def _pending_rows(limit=2):
    """找出「服务端有新消息、本地一条正文都没有」的会话（外部群/久未打开的私聊）。"""
    import sqlite3
    base = os.path.dirname(os.path.abspath(__file__))
    sess = os.path.join(base, '_wxdec', 'session_plain.db', 'session.db')
    msg = os.path.join(base, '_wxdec', 'msg_plain.db', 'message.db')
    if not (os.path.exists(sess) and os.path.exists(msg)):
        return []
    with sqlite3.connect(msg) as c:
        have = {r[0] for r in c.execute('select distinct conversation_id from message_table')}
    out = []
    with sqlite3.connect(sess) as c:
        for cid, name, last_id in c.execute(
                'select id, name, last_message_id from conversation_table '
                'where last_message_id>0 order by last_message_id desc limit 80'):
            if cid in have or not name:
                continue
            if name in ('文件传输助手', '微盘', '日历', '公告', '企业通知'):
                continue
            out.append({'src': '企微', 'kind': 'group' if str(cid)[:2] in ('R:', 'M:') else 'direct',
                        'conv': name, 'sender': '',
                        'text': '有未读消息（正文未缓存，客户端里打开一次即同步）',
                        'time': '--:--', 'ts': 0, 'pending': True})
            if len(out) >= limit:
                break
    return out


def _unread_counts():
    """{(conv_id, unread_count)} —— 来自 session.db 的 unread_conversation_table。"""
    import sqlite3
    base = os.path.dirname(os.path.abspath(__file__))
    sess = os.path.join(base, '_wxdec', 'session_plain.db', 'session.db')
    if not os.path.exists(sess):
        return None            # 表不可用（未解密/未登录），调用方可回退到时间窗

    with sqlite3.connect(sess) as c:
        names = dict(c.execute('select id, name from conversation_table').fetchall())
        lasts = dict(c.execute('select id, last_message_time from conversation_table').fetchall())
        rows = c.execute('select conversation_id, unread_count from unread_conversation_table '
                         'where unread_count>0').fetchall()
    out = {}
    for cid, n in rows:
        out[cid] = (names.get(cid) or '', int(n), int(lasts.get(cid) or 0))
    return out


def unread_rows_by_unread(limit=4, max_age_h=48):
    """真实未读：unread_count>0 的会话 + 最新一条消息（任何类型）做预览。

    返回 None 表示「未读表不可用」（未解密/未登录），调用方可以回退到时间窗；
    返回 [] 表示「表可用且确实没有未读」，调用方不得再回退。

    预览取「最新一条消息」而不是「最新一条文本」：企微的未读计数覆盖所有消息类型，
    只看类型 2 会把三天前的文字当成刚到的未读（2026-11-19 的实际事故）。
    最新消息不是文本时给类型占位，不伪造正文。
    超过 max_age_h 小时的未读只在日志里计数，不进面板（客户端 unread_count 不随手机端已读同步，会留陈旧未读）。
    """
    counts = _unread_counts()
    if counts is None:
        return None
    p = os.path.join(HERE, 'wxwork_chat.json')
    if not os.path.exists(p):
        # 契约：返回 None = 数据源不可用（调用方可回退）；[] 只能表示「确实没有未读」
        print('WXU reason=no-chat（chat.json 缺失，按不可用处理）')
        return None
    chat = json.load(open(p, encoding='utf-8'))
    by_id = {}
    for c in chat.get('conversations', []):
        by_id[c.get('id')] = c
    now = dt.datetime.now().timestamp()
    stale = 0
    out = []
    for cid, (name, cnt, last_t) in counts.items():
        c = by_id.get(cid) or {}
        name = (name or c.get('name') or '').strip()
        if not name:
            continue                      # 公众号/机器人/无名内部会话，不展示
        if name in BOT_CONV or BOT_RE.search(name):
            continue
        kind = c.get('kind') or ('group' if str(cid)[:2] in ('R:', 'M:') else 'direct')
        if kind not in ('group', 'direct'):
            continue
        msgs = [m for m in (c.get('msgs') or []) if m.get('ts')]
        msgs.sort(key=lambda m: m['ts'])
        last = msgs[-1] if msgs else None
        text = ''
        ts = 0
        if last:
            ts = last['ts'] or 0
            if last.get('type') == 2:
                text = _clean_text((last.get('text') or '').strip())
            if len(text) < 2:
                text = (last.get('label') or '').strip() or _type_label(last.get('type'))
        if len(text) < 2:
            text = '有 %d 条未读（正文未缓存，客户端里打开一次即同步）' % cnt
        if SYS_RE.search(text) or BOT_TEXT.search(text):
            continue
        ref_ts = max(ts, last_t or 0)      # 会话表 last_message_time 与消息表交叉，任一新鲜就不算陈旧
        if ref_ts and now - ref_ts > max_age_h * 3600:
            stale += 1
            continue                      # 陈旧未读：只在日志里体现，别让它冒充今天的消息
        out.append({
            'ts': ts,
            't': _tlabel(ts) if ts else '--:--',
            'sender': ((last or {}).get('from') or name)[:10],
            'text': _clip(text, 52),
            'model': _model_of(text, name),
            'conv': name,
            'src': '企微',
            'kind': kind,
            'unread': cnt,
        })
    out.sort(key=lambda r: -r['ts'])
    return out[:limit], stale


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--hours', type=int, default=3,
                    help='回退路径（未读表不可用时）的时间窗；主路径用 --max-age-hours')
    ap.add_argument('--max-age-hours', type=int, default=48,
                    help='未读预览超过这个小时数就不进面板（客户端 unread_count 不跟手机端已读同步）')
    ap.add_argument('--json', default='')
    a = ap.parse_args()

    key = read_key()
    if not key:
        # 没密钥等于未读不可用，必须让管道看见（exit 0 会让它记成 ok）
        print('WXU FAIL reason=no-key（企微库密钥缺失或客户端未登录）')
        return 3
    ok_plain = refresh_plain(key)
    ok_chat, tail = build_chat()
    degraded = (not ok_plain) or (not ok_chat)
    max_age_h = a.max_age_hours if a.max_age_hours else max(48, a.hours)
    real = unread_rows_by_unread(max_age_h=max_age_h)
    stale = 0
    if real is None:                       # 未读表不可用才回退到时间窗
        # unread_rows 在任何失败路径上都返回 None（无解密器 / 库打不开 / SQL 变），
        # 直接相加会 TypeError，把「不可用」变成「崩掉」。按空列表处理并保留 degraded 标记。
        rows = (unread_rows(a.hours) or []) + _pending_rows()
    else:
        rows, stale = real                 # 表可用：0 条就是没有未读，绝不回退

    if a.json:
        with open(a.json, 'w', encoding='utf-8') as f:
            json.dump({'at': dt.datetime.now().strftime('%Y-%m-%d %H:%M'), 'rows': rows,
                       'degraded': degraded,
                       'reason': '' if not degraded else ('no-decryptor' if wdec is None else ('plain=0' if not ok_plain else 'chat=0'))},
                      f, ensure_ascii=False, indent=1)
    print('WXU plain=%s chat=%s rows=%d stale=%d degraded=%s %s'
          % (ok_plain, ok_chat, len(rows), stale, degraded, (tail[0] if tail else '')[:80]))
    for r in rows[:3]:
        print('   %s %-14s %s' % (r['t'], r['sender'], r['text'][:40]))
    return 0


if __name__ == '__main__':
    sys.exit(main())
