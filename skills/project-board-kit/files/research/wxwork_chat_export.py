"""企微本地库 → 群聊/私聊消息导出（只读，输入为已解密库）。

输入：
  <dec>/session_plain.db/session.db   conversation_table / conversation_user_table / conversation_member_nickname_table
  <dec>/msg_plain.db/message.db       message_table（content 为 protobuf）
  <dec>/user_plain.db/user.db         （可选）联系人姓名
输出：
  research/wxwork_chat.json           {at, conversations:[{id,name,kind,members,last,msgs:[{t,from,text,type}]}]}
用法：
  python wxwork_chat_export.py --dec research/_wxdec --out research/wxwork_chat.json [--days 14]

content 解析：文本消息 content_type=2，正文在 protobuf 嵌套字段
  外层 field1(bytes) -> 内层 field2(bytes) -> 最内 field1(string)
  其他类型只保留可读文本（撤回/系统提示等），解析失败则留空。
"""
import re
import argparse
import datetime as dt
import json
import os
import sqlite3
import sys


def read_varint(b, i):
    shift = 0
    val = 0
    while i < len(b):
        c = b[i]
        i += 1
        val |= (c & 0x7F) << shift
        if not c & 0x80:
            return val, i
        shift += 7
    return None, i


def fields(b):
    """protobuf 顶层字段 → [(field_no, wire_type, value_bytes_or_int)]"""
    out = []
    i = 0
    while i < len(b):
        key, i = read_varint(b, i)
        if key is None:
            break
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = read_varint(b, i)
            out.append((fn, wt, v))
        elif wt == 2:
            ln, i = read_varint(b, i)
            if ln is None or i + ln > len(b):
                break
            out.append((fn, wt, b[i:i + ln]))
            i += ln
        elif wt == 1:
            out.append((fn, wt, b[i:i + 8]))
            i += 8
        elif wt == 5:
            out.append((fn, wt, b[i:i + 4]))
            i += 4
        else:
            break
    return out


def as_text(b):
    try:
        return b.decode('utf-8')
    except Exception:
        return None


def fallback_text(raw):
    """从任意消息体里抽出可读文本（用于分段/富文本/带 @ 的消息）。"""
    try:
        txt = raw.decode("utf-8", "ignore")
    except Exception:
        return ""
    parts = []
    cur = []
    for ch in txt:
        if ch.isprintable() and ch != "\ufffd":
            cur.append(ch)
        else:
            if len(cur) >= 2:
                parts.append("".join(cur))
            cur = []
    if len(cur) >= 2:
        parts.append("".join(cur))
    out, seen = [], set()
    for p in parts:
        p = p.strip()
        if not p or p in seen:
            continue
        if not any(c.isalnum() or "\u4e00" <= c <= "\u9fff" for c in p):
            continue
        seen.add(p)
        out.append(p)
    return " ".join(out)[:400]


def msg_text(body, ctype):
    """尽量取出可读正文。"""
    if not body:
        return ''
    if ctype == 2:
        f1 = [v for (n, w, v) in fields(body) if n == 1 and w == 2]
        if f1:
            inner = fields(f1[0])
            f2 = [v for (n, w, v) in inner if n == 2 and w == 2]
            if f2:
                inner2 = fields(f2[0])
                f1b = [v for (n, w, v) in inner2 if n == 1 and w == 2]
                t = as_text(f1b[0]) if f1b else as_text(f2[0])
                if t is not None:
                    return t
    # 兜底：整体当字符串
    return as_text(body) or fallback_text(body)


# 消息类型 -> 人类可读种类（2026-11-19 按本机 message.db 实测确认）
# 系统事件/空体：不进面板也不值得报警，避免 UNKNOWN 每天刷屏
SYS_QUIET = (503, 673, 1002, 1011, 2101)

CT_KIND = {
    0: '系统消息', 2: '文本', 4: '名片', 14: '图片', 15: '文件', 20: '文件', 22: '视频', 23: '视频',
    28: '邮件提醒', 31: '资讯', 34: '文件', 38: '通知', 40: '通话', 101: '图片',
    102: '文件', 103: '视频', 123: '图片', 561: '打卡', 565: '微盘文件', 573: '提醒',
    675: '待办', 1022: '系统',
}
MEDIA_CT = (14, 15, 20, 22, 23, 34, 101, 102, 103, 123)
FEXT = ('pptx', 'ppt', 'docx', 'doc', 'xlsx', 'xls', 'pdf', 'zip', 'rar', '7z', 'txt',
        'csv', 'png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'mp4', 'mov', 'avi', 'mp3',
        'wav', 'dwg', 'step', 'stp', 'igs', 'prt', 'ai', 'psd', 'ttf', 'exe', 'apk',
        'html', 'md', 'json')


def _strs(b, depth=0, out=None):
    """递归收集 protobuf 树里的可打印字符串（限深 3 层）。"""
    if out is None:
        out = []
    if depth > 3:
        return out
    for n, w, v in fields(b):
        if w != 2:
            continue
        t = as_text(v)
        if t and t.isprintable():
            out.append(t)
        elif not t:
            _strs(v, depth + 1, out)
    return out


def _is_fname(t):
    if t.startswith(('http://', 'https://', '//', 'www.')):
        return False   # CDN 图标地址不是文件名（微盘消息里会把图标 URL 带进来）
    if not 3 <= len(t) <= 120 or '\n' in t:
        return False
    parts = t.lower().rsplit('.', 1)
    return len(parts) == 2 and parts[1] in FEXT


def _human(n):
    if not n or n < 1024:
        return ''
    v = float(n)
    for unit in ('KB', 'MB', 'GB'):
        v /= 1024.0
        if v < 1024:
            return ('%d %s' % (v, unit)) if v >= 10 else ('%.1f %s' % (v, unit))
    return '%d TB' % v


def msg_meta(body, ctype):
    """非文本消息的种类 / 文件名 / 体积，用来如实显示「这是 PPT 还是图片」。

    实测（message.db 2026-11-19）：20 / 15 / 34 / 102 是文件，名字在顶层字段 2 且带扩展名，
    体积在字段 4；14 是图片（字段 2 = 企业微信截图_xxx.png）；101 / 123 也是图片；
    22 / 23 / 103 是视频（.mp4 名）；565 是微盘文件（名字在字段 5）。
    """
    meta = {'kind': CT_KIND.get(ctype, ''), 'name': '', 'size': 0}
    if not body or ctype == 2:
        return meta
    if ctype in MEDIA_CT or ctype == 565:
        names = [t for t in _strs(body) if _is_fname(t)]
        if names:
            names.sort(key=len, reverse=True)
            nm = names[0].strip()
            if meta['kind'] == '图片' and ' ' in nm:
                # 字段 2 常把说明文字和文件名拼一起（「资源 1.png」），只留最后一段文件名
                tail = [t for t in nm.split() if _is_fname(t)]
                if tail:
                    nm = tail[-1]
            meta['name'] = nm
        top = dict((n, v) for (n, w, v) in fields(body) if w == 0)
        size = top.get(4) or top.get(3) or 0
        if isinstance(size, int) and 0 < size < (1 << 42):
            meta['size'] = size
    if not meta['size']:
        meta['size'] = size_from_text(body)   # 微盘文件把体积写成文本（「3 MB」）
    return meta


SIZE_RE = re.compile(r'^(\d+(?:\.\d+)?)\s*(B|KB|MB|GB)$', re.I)
SIZE_UNIT = {'B': 1, 'KB': 1024, 'MB': 1024 ** 2, 'GB': 1024 ** 3}


def size_from_text(body):
    """微盘文件把体积写成文本字段（「3 MB」）；能转成字节数就转，转不了返回 0。"""
    for t in _strs(body):
        m = SIZE_RE.match(t.strip())
        if m:
            return int(float(m.group(1)) * SIZE_UNIT[m.group(2).upper()])
    return 0


def msg_label(ctype, meta, text=''):
    """面板预览：能拿到文件名就给文件名，拿不到就只给种类。"""
    if ctype == 2:
        return text
    kind = meta.get('kind') or ''
    if not kind:
        return '[非文本消息]'
    name = meta.get('name') or ''
    if not name:
        return '[%s]' % kind
    size = _human(meta.get('size') or 0)
    return '[%s] %s%s' % (kind, name, (' (%s)' % size) if size else '')

def load_sessions(path):
    con = sqlite3.connect(path)
    cons = {}
    for cid, name, nid, last in con.execute(
            'select id,name,con_numeric_id,last_message_time from conversation_table'):
        cons[cid] = {'id': cid, 'name': name or '', 'nid': nid, 'last': last or 0,
                     'members': []}
    nick = {}
    for uid, nn in con.execute('select userid,nickname from conversation_member_nickname_table'):
        nick[uid] = nn
    for cid, uid, nn in con.execute(
            'select conversation_id,user_id,nick_name from conversation_user_table'):
        if cid in cons:
            nm = nn or nick.get(uid) or ''
            cons[cid]['members'].append((uid, nm))
    con.close()
    return cons


def load_users(path):
    """user.db 表结构随版本变化，做宽松映射 userid -> 显示名。"""
    users = {}
    if not os.path.exists(path):
        return users
    con = sqlite3.connect(path)
    tabs = [r[0] for r in con.execute("select name from sqlite_master where type='table'")]
    for t in tabs:
        cols = [c[1] for c in con.execute('pragma table_info("%s")' % t)]
        idc = next((c for c in cols if c.lower() in ('user_id', 'userid', 'vid', 'id')), None)
        nc = next((c for c in cols if c.lower() in ('name', 'real_name', 'nickname', 'user_name')), None)
        if not idc or not nc:
            continue
        try:
            for uid, nm in con.execute('select "%s","%s" from "%s"' % (idc, nc, t)):
                if uid and nm:
                    users[uid] = nm
        except Exception:
            continue
    con.close()
    return users


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--dec', default='research/_wxdec')
    ap.add_argument('--out', default='research/wxwork_chat.json')
    ap.add_argument('--days', type=int, default=14)
    ap.add_argument('--max-per', type=int, default=200)
    a = ap.parse_args()

    sp = os.path.join(a.dec, 'session_plain.db', 'session.db')
    mp = os.path.join(a.dec, 'msg_plain.db', 'message.db')
    up = os.path.join(a.dec, 'user_plain.db', 'user.db')
    for p in (sp, mp):
        if not os.path.exists(p):
            print('MISSING', p)
            return 2

    cons = load_sessions(sp)
    users = load_users(up)
    con = sqlite3.connect(mp)
    rows = con.execute(
        'select conversation_id,sender_id,send_time,content_type,content from message_table '
        'order by send_time desc').fetchall()
    con.close()

    cutoff = (dt.datetime.now() - dt.timedelta(days=a.days)).timestamp()
    by = {}
    unk = {}
    for cid, sid, ts, ct, body in rows:
        if cid not in cons:
            continue
        if ts and ts < cutoff:
            continue
        if ct != 2 and ct not in CT_KIND and ct not in SYS_QUIET:
            unk[ct] = unk.get(ct, 0) + 1       # 未登记类型：只在报告里提示，不猜种类
        lst = by.setdefault(cid, [])
        if len(lst) >= a.max_per:
            continue
        meta = msg_meta(body, ct)
        item = {
            't': dt.datetime.fromtimestamp(ts).strftime('%Y-%m-%d %H:%M') if ts else '',
            'ts': ts,
            'from': users.get(sid) or str(sid),
            'type': ct,
            # 非文本消息不塞解出来的乱码串：正文留给 label，text 只放真正的文本
            'text': msg_text(body, ct) if ct == 2 else '',
            'label': msg_label(ct, meta) if (meta['kind'] or (body and ct not in (0,))) else '[系统消息]',
        }
        if meta['kind']:
            item['kind'] = meta['kind']
        if meta['name']:
            item['name'] = meta['name']
        if meta['size']:
            item['size'] = meta['size']
        lst.append(item)

    out = []
    for cid, c in cons.items():
        msgs = sorted(by.get(cid, []), key=lambda x: x['ts'] or 0)
        nm = (c['name'] or '').strip()
        mem = len(c['members'])
        sid = str(cid)
        # 企微会话 id 约定：S:<peer>_<me> 单聊；以 M 或 R 开头的为多人群聊（含内部群）
        if sid.startswith('S:'):
            kind = 'direct'
            if not nm:
                peer = sid[2:].split('_')[0]
                nm = str(users.get(peer) or users.get(peer.strip()) or '').strip()
        elif sid[:2] in ('M:', 'R:') or '@chatroom' in sid:
            # 无名且无成员的多人群 = 内部/机器人会话，无法判定归属，归 unknown 不给角标
            kind = 'group' if (nm or mem) else 'unknown'
        elif mem > 2:
            kind = 'group'
        elif nm:
            kind = 'direct'
        else:
            kind = 'unknown'
        out.append({
            'id': cid,
            'name': c['name'],
            'kind': kind,
            'members': [n for _, n in c['members'] if n][:50],
            'last': dt.datetime.fromtimestamp(c['last']).strftime('%Y-%m-%d %H:%M') if c['last'] else '',
            'msgs': msgs,
        })
    out.sort(key=lambda c: c['msgs'][-1]['ts'] if c['msgs'] else 0, reverse=True)

    payload = {
        'at': dt.datetime.now().strftime('%Y-%m-%d %H:%M'),
        'source': 'WXWork local message.db / session.db',
        'days': a.days,
        'conversations': out,
    }
    os.makedirs(os.path.dirname(os.path.abspath(a.out)) or '.', exist_ok=True)
    with open(a.out, 'w', encoding='utf-8') as f:
        json.dump(payload, f, ensure_ascii=False, indent=1)

    groups = [c for c in out if c['kind'] == 'group']
    directs = [c for c in out if c['kind'] == 'direct']
    nmsg = sum(len(c['msgs']) for c in out)
    if unk:
        print('UNKNOWN ct=%s （未登记类型，面板显示 [非文本消息]；确认后补进 CT_KIND）'
              % ','.join('%d:%d' % (k, unk[k]) for k in sorted(unk)))
    print('CHAT OK convs=%d group=%d direct=%d msgs=%d out=%s' %
          (len(out), len(groups), len(directs), nmsg, a.out))
    for c in out[:5]:
        t = c['msgs'][-1]['text'][:40] if c['msgs'] else ''
        print('  %-2s %-24s msgs=%-4d last=%s | %s' %
              (c['kind'][:1], c['name'][:24], len(c['msgs']), c['msgs'][-1]['t'] if c['msgs'] else '-', t))
    return 0


if __name__ == '__main__':
    sys.exit(main())
