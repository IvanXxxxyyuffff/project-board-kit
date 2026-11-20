"""企微日历导出（只读、离线）：解密 calendar_r7.db 并解析日程。

链路：复制库文件 → wxwork_db_decrypt.py 解密 → 读 calendar_main_table_v2 →
从 rawdata(protobuf) 里取标题 / 组织者 / 参会人 / 地点 → 写 research/wxwork_calendar.json

用法：
    python wxwork_calendar.py            # 自动找账号目录并导出近 60 天
    python wxwork_calendar.py --days 90
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
KEY_FILE = os.path.join(HERE, "_wxdec", "_key.txt")
OUT = os.path.join(HERE, "wxwork_calendar.json")
DECRYPT = os.path.join(HERE, "wxwork_db_decrypt.py")

CJK = re.compile(r"[\u4e00-\u9fa5]")
EMAIL = re.compile(r"^[\w.\-]+@[\w.\-]+$")
ROOM_HINT = re.compile(r"(会议室|Room|room|楼|层|室)")


def find_db() -> str | None:
    """定位 calendar_r7.db（任意登录账号都只有一个在用）。"""
    root = os.path.join(os.path.expanduser("~"), "Documents", "WXWork")
    if not os.path.isdir(root):
        return None
    best = None
    for name in os.listdir(root):
        cand = os.path.join(root, name, "Data", "calendar_r7.db")
        if os.path.isfile(cand):
            mtime = os.path.getmtime(cand)
            if best is None or mtime > best[0]:
                best = (mtime, cand)
    return best[1] if best else None


def read_key() -> str | None:
    if not os.path.isfile(KEY_FILE):
        return None
    text = open(KEY_FILE, "r", encoding="utf-8").read().strip()
    return text if re.fullmatch(r"[0-9a-fA-F]{32}", text) else None


def decrypt(src: str, key: str, workdir: str) -> str:
    """调用现有解密器，返回解密后的库路径。"""
    work = workdir or os.path.join(HERE, "_wxdec")
    os.makedirs(work, exist_ok=True)
    tmp = os.path.join(work, "src_cal.db")
    shutil.copy2(src, tmp)
    outdir = os.path.join(work, "cal_out")
    subprocess.run(
        [sys.executable, "-X", "utf8", "wxwork_db_decrypt.py", tmp, key, outdir],
        cwd=HERE, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    plain = os.path.join(outdir, "src_cal.db")
    if not os.path.isfile(plain):
        raise SystemExit("DECRYPT_FAIL 未生成明文库")
    return plain


def strings(blob: bytes) -> list[str]:
    """从 protobuf 字节里抽可读字符串（UTF-8 连续可打印段）。"""
    out = []
    for chunk in re.findall(rb"[\x20-\x7e]{2,}", blob):
        try:
            out.append(chunk.decode("utf-8").strip())
        except UnicodeDecodeError:
            continue
    whole = blob.decode("utf-8", "ignore")
    for chunk in re.findall(r"[\u4e00-\u9fa5][\u4e00-\u9fa5\w：:，,。.\- ]{2,}", whole):
        out.append(chunk.strip())
    return out


def parse_row(row: tuple) -> dict | None:
    idx, sid, flag, start, end, remind, sign, raw, source = row
    if not start or not end:
        return None
    if not isinstance(raw, (bytes, bytearray)):
        return None
    items = strings(bytes(raw))
    emails = [s for s in items if EMAIL.match(s)]
    titles = [s for s in items if CJK.search(s) and len(s) >= 3 and not EMAIL.match(s)]
    rooms = [s for s in titles if ROOM_HINT.search(s)]
    # 标题：优先非会议室描述里最短的一条（企微把标题放前面，参会人昵称在后）
    title = None
    for cand in titles:
        if rooms and cand in rooms:
            continue
        title = cand
        break
    if title is None and titles:
        title = titles[0]
    return {
        "id": sid,
        "start": dt.datetime.fromtimestamp(int(start)).strftime("%Y-%m-%d %H:%M"),
        "end": dt.datetime.fromtimestamp(int(end)).strftime("%Y-%m-%d %H:%M"),
        "title": title or "(未命名日程)",
        "place": rooms[0] if rooms else "",
        "attendees": emails[:20],
        "names": [s for s in titles if s != title][:12],
        "source": source or "",
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--days", type=int, default=60)
    ap.add_argument("--out", default=OUT)
    args = ap.parse_args()

    key = read_key()
    if not key:
        print("KEY_MISSING 先运行 research\\wxwork_enable.ps1")
        return 2
    db = find_db()
    if not db:
        print("DB_MISSING 未找到 calendar_r7.db")
        return 3

    workdir = tempfile.mkdtemp(prefix="wxcal_")
    try:
        plain = decrypt(db, key, workdir)
        conn = sqlite3.connect(plain)
        rows = conn.execute(
            "select idx_id, schedule_id, flag, starttime, endtime,"
            " recent_remindtime, sign_remindtime, rawdata, source_id"
            " from calendar_main_table_v2 order by starttime"
        ).fetchall()
        conn.close()
    finally:
        shutil.rmtree(workdir, ignore_errors=True)

    today = dt.date.today()
    lo = today - dt.timedelta(days=args.days)
    hi = today + dt.timedelta(days=args.days)
    events = []
    for row in rows:
        item = parse_row(row)
        if not item:
            continue
        day = dt.date.fromisoformat(item["start"][:10])
        if not (lo <= day <= hi):
            continue
        events.append(item)

    payload = {
        "at": dt.datetime.now().strftime("%Y-%m-%d %H:%M"),
        "source": "wxwork calendar_r7.db (只读离线解密)",
        "window": f"{lo} ~ {hi}",
        "count": len(events),
        "items": events,
    }
    with open(args.out, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, ensure_ascii=False, indent=1)
    print(f"CAL OK events={len(events)} rows={len(rows)} -> {args.out}")
    for ev in events[:5]:
        print(f"  {ev['start']} ~ {ev['end'][11:]}  {ev['title']}  {('@' + ev['place']) if ev['place'] else ''}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
