"""从企微本地库导出让「待办」列表（只读、零注入）。

数据源：`Documents\\WXWork\\<账号>\\Data\\forever_store.db` 的 `todo_v4` 表
（列 content / status / create_time / update_time / remind_timestamp），
该库与 message.db 同一把 wxSQLite3 密钥，可直接离线解密。

输出：`research/wxwork_todo.json`，供 `wxwork_todo_merge.py` 并入部门待办。
status：4 视为已完成/已关闭（不导出），其余视为未完成。
用法：python wxwork_todo_local.py [--out PATH] [--all]
"""
import argparse
import datetime as dt
import json
import os
import shutil
import sqlite3
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
try:
    import wxwork_db_decrypt as W
except Exception:  # 公开套件不含解密器；调用方按「通道停用」处理
    W = None

CLOSED_STATUS = 4


def read_key():
    p = os.path.join(HERE, "_wxdec", "_key.txt")
    if not os.path.exists(p):
        return None
    h = open(p, encoding="utf-8").read().strip()
    try:
        return bytes.fromhex(h)
    except ValueError:
        return None


def _roots():
    """企微数据目录的候选位置（含文档目录被重定向到 OneDrive 的情况）。"""
    home = os.path.expanduser("~")
    out = []
    for base in (
        os.path.join(home, "Documents", "WXWork"),
        os.path.join(home, "OneDrive", "Documents", "WXWork"),
        os.path.join(home, "OneDrive", "文档", "WXWork"),
        os.path.join(os.environ.get("USERPROFILE", home), "Documents", "WXWork"),
        os.path.join(os.environ.get("WXWORK_DATA", ""), ""),
    ):
        if base and os.path.isdir(base) and base not in out:
            out.append(base)
    return out


def find_store():
    """返回最新修改的 forever_store.db（企微本地待办库）。"""
    best = None
    for base in _roots():
        for acc in os.listdir(base):
            p = os.path.join(base, acc, "Data", "forever_store.db")
            if os.path.exists(p):
                if best is None or os.path.getmtime(p) > os.path.getmtime(best):
                    best = p
    return best


def export(out_path, include_closed=False, days=180):
    key = read_key()
    if not key:
        print("TODO_LOCAL fail reason=no_key")
        return 2
    src = find_store()
    if not src:
        print("TODO_LOCAL fail reason=no_db")
        return 2
    tmp = os.path.join(tempfile.gettempdir(), "wx_fs.db")
    dst = os.path.join(tempfile.gettempdir(), "wx_fs_plain.db")
    shutil.copy2(src, tmp)
    if W is None:
        print("TODO_LOCAL fail reason=no-decryptor（本套件不含 wxwork_db_decrypt.py）")
        return 2
    pages = W.decrypt_db(tmp, dst, key)
    if not pages:
        print("TODO_LOCAL fail reason=decrypt")
        return 2

    items = []
    c = sqlite3.connect(dst)
    for tid, content, status, ct, ut, rt, creator in c.execute(
            "select id, content, status, create_time, update_time, remind_timestamp, creator_id "
            "from todo_v4 order by update_time desc"):
        text = content.decode("utf-8", "ignore") if isinstance(content, (bytes, bytearray)) else str(content or "")
        text = " ".join(text.split())
        if not text:
            continue
        if not include_closed and int(status or 0) == CLOSED_STATUS:
            continue
        if days and ut:
            import time as _t
            if int(ut) < _t.time() - days * 86400:
                continue
        due = ""
        if rt:
            try:
                due = dt.datetime.fromtimestamp(int(rt)).strftime("%Y-%m-%d")
            except Exception:
                due = ""
        else:
            try:
                due = dt.datetime.fromtimestamp(int(ut or ct or 0)).strftime("%Y-%m-%d")
            except Exception:
                due = ""
        items.append({
            "id": str(tid),
            "title": text[:120],
            "status": int(status or 0),
            "due": due,
            "creator": str(creator or ""),
            "update": dt.datetime.fromtimestamp(int(ut or ct or 0)).strftime("%Y-%m-%d %H:%M") if (ut or ct) else "",
        })

    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump({"at": dt.datetime.now().strftime("%Y-%m-%d %H:%M"), "source": "forever_store.todo_v4",
                   "items": items}, f, ensure_ascii=False, indent=1)
    print("TODO_LOCAL ok items=%d pages=%s out=%s" % (len(items), pages, out_path))
    for it in items[:4]:
        print("   ", it["due"], it["title"][:40])
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=os.path.join(HERE, "wxwork_todo.json"))
    ap.add_argument("--all", action="store_true", help="含已完成/已关闭")
    ap.add_argument("--days", type=int, default=180, help="只导出最近 N 天更新过的")
    a = ap.parse_args()
    sys.exit(export(a.out, a.all, a.days))


if __name__ == "__main__":
    main()
