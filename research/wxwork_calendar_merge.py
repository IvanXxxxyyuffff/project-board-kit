"""把企微日历事件并入挂件已读取的 meeting_data.json（无需改 Rust 侧）。

挂件「会议安排」标签读的是 <data 同目录>/meeting_data.json，结构：
  {at, source, today:{label,items:[{id,date,start,end,name,place,people,status}]},
   week:{...}, month:{...}, all, window}
本脚本读 wxwork_calendar.json（企微日历，只读离线解密产物），把条目按日期归到
today/week/month 三档，用 name 前缀「〔日历〕」区分来源，按时间排序后写回。

用法：
    python wxwork_calendar_merge.py            # 合并并写回
    python wxwork_calendar_merge.py --dry-run  # 只打印不落盘
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
CAL = os.path.join(HERE, "wxwork_calendar.json")
MEET = os.path.join(HERE, "meeting_data.json")
PREFIX = "〔日历〕"


def load(path: str) -> dict:
    with open(path, "r", encoding="utf-8") as fh:
        return json.load(fh)


def rows_from_calendar(cal: dict) -> list[dict]:
    out = []
    for item in cal.get("items", []):
        start = str(item.get("start", ""))
        end = str(item.get("end", ""))
        if len(start) < 16:
            continue
        day = start[:10]
        out.append({
            "id": "cal:" + str(item.get("id", ""))[:24],
            "date": day,
            "start": start[11:16],
            "end": end[11:16] if len(end) >= 16 else "",
            "name": PREFIX + str(item.get("title", "")).rstrip("0123456789 "),
            "place": item.get("place", "") or "",
            "people": "、".join(item.get("names", [])[:4]),
            "status": "cal",
        })
    return out


def bucket(rows: list[dict], today: dt.date) -> dict:
    week_start = today - dt.timedelta(days=today.weekday())
    week_end = week_start + dt.timedelta(days=6)
    month_end = dt.date(today.year, today.month, 28) + dt.timedelta(days=4)
    month_end = dt.date(month_end.year, month_end.month, 1) - dt.timedelta(days=1)
    out = {"today": [], "week": [], "month": []}
    for row in rows:
        try:
            day = dt.date.fromisoformat(row["date"])
        except ValueError:
            continue
        if day == today:
            out["today"].append(row)
        if week_start <= day <= week_end:
            out["week"].append(row)
        if today.replace(day=1) <= day <= month_end:
            out["month"].append(row)
    return out


def merge_section(section: dict, extra: list[dict]) -> int:
    items = list(section.get("items", []))
    seen = {(i.get("date"), i.get("start"), i.get("name")) for i in items}
    added = 0
    for row in extra:
        key = (row["date"], row["start"], row["name"])
        if key in seen:
            continue
        items.append(row)
        seen.add(key)
        added += 1
    items.sort(key=lambda i: (str(i.get("date", "")), str(i.get("start", ""))))
    section["items"] = items
    return added


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if not os.path.isfile(CAL):
        print("CAL_MISSING 先跑 wxwork_calendar.py")
        return 2
    if not os.path.isfile(MEET):
        print("MEET_MISSING 先跑 ec_meeting_fetch.py")
        return 3

    cal = load(CAL)
    meet = load(MEET)
    rows = rows_from_calendar(cal)
    if not rows:
        print("CAL_EMPTY 没有可合并的日历条目（正常：缓存无未来日程）")
        return 0

    groups = bucket(rows, dt.date.today())
    added = {k: merge_section(meet.setdefault(k, {"items": []}), v) for k, v in groups.items()}
    meet["calendarAt"] = cal.get("at", "")

    print(f"MERGE calendar={len(rows)} added today={added['today']} week={added['week']} month={added['month']}")
    for key in ("today", "week"):
        for it in meet[key]["items"][:4]:
            tag = "日历" if it["name"].startswith(PREFIX) else "EM  "
            print(f"  [{dict(today='当日', week='本周').get(key, key)}] {tag} {it['date']} {it.get('start','')} {it['name'][:28]}")

    if args.dry_run:
        print("DRY-RUN 未写盘")
        return 0

    backup = MEET + ".bak-cal"
    if os.path.isfile(backup) is False:
        with open(backup, "w", encoding="utf-8") as fh:
            json.dump(load(MEET), fh, ensure_ascii=False, indent=1)
    with open(MEET, "w", encoding="utf-8") as fh:
        json.dump(meet, fh, ensure_ascii=False, indent=1)
    print("MERGE_WRITTEN " + MEET)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
