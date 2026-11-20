"""企微待办被动取证（只读内存，不产生任何输入事件）。

原理：企微登录后会在后台拉取待办列表；只要能捕捉到内存中出现的待办负载，
就能离线落盘，不依赖任何点击/注入。

流程：
  1. 用 memfind 在 WXWork 进程里找标记（todo_id / todo_title / deadline）
  2. 命中后把上下文（含相邻中文串）收集起来
  3. 用正则抽「标题 + 时间」，写成 research/wxwork_todo.json

用法：
    python wxwork_todo_capture.py            # 扫一轮，命中即写盘
    python wxwork_todo_capture.py --rounds 12 --interval 30   # 后台等 6 分钟

注意：只调用 ReadProcessMemory 侧的 wxwork_memfind.exe；不写内存、不发输入。
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MEMFIND = os.path.join(HERE, "wxwork_memfind.exe")
OUT = os.path.join(HERE, "wxwork_todo.json")
MARKERS = ("todo_id", "todo_title", "todoidlist")

DATE_RE = re.compile(r"20\d{2}[-/年]\d{1,2}[-/月]\d{1,2}")
TITLE_RE = re.compile(r"[\u4e00-\u9fa5][\u4e00-\u9fa5A-Za-z0-9（）()·\-_ ]{3,40}")


def wxwork_pid() -> int | None:
    """找企微主进程（内存占用最大的那个）。"""
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "Get-Process -Name WXWork -ErrorAction SilentlyContinue |"
             " Sort-Object WorkingSet64 -Descending | Select-Object -First 1 -ExpandProperty Id"],
            capture_output=True, text=True, timeout=20,
        ).stdout.strip()
        return int(out) if out else None
    except Exception:
        return None


def logged_in() -> bool:
    """企微登录后至少有一个 WXWorkWeb(CEF) 子进程；没有就是二维码/登录页。"""
    try:
        out = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "(Get-Process -Name WXWorkWeb -ErrorAction SilentlyContinue | Measure-Object).Count"],
            capture_output=True, text=True, timeout=15,
        ).stdout.strip()
        return out.isdigit() and int(out) > 0
    except Exception:
        return False


def scan_marker(pid: int, marker: str) -> str:
    """调用只读扫描器，返回原始输出（命中时含 ctx 行）。"""
    try:
        res = subprocess.run([MEMFIND, marker, "2", str(pid)],
                             capture_output=True, text=True, timeout=300, errors="ignore")
        return res.stdout or ""
    except subprocess.TimeoutExpired:
        return ""


def cjk_ratio(text: str) -> float:
    if not text:
        return 0.0
    cjk = sum(1 for ch in text if "\u4e00" <= ch <= "\u9fa5")
    return cjk / max(1, len(text))



CJK_RE = re.compile(r"[\u4e00-\u9fff]")
PRIVATE_RE = re.compile(r"[\ue000-\uf8ff]")


def _mojibake(s):
    """单字节 ASCII 被按 UTF-16LE 读成汉字：反向 encode 回去应全是可打印 ASCII。"""
    try:
        raw = s.encode("utf-16-le", errors="ignore")
    except Exception:
        return False
    if len(raw) < 4:
        return False
    printable = sum(1 for b in raw if 0x20 <= b < 0x7F)
    return printable / len(raw) > 0.85


def _looks_noise(s):
    s = (s or "").strip()
    if len(s) < 2 or len(s) > 60:
        return True
    if PRIVATE_RE.search(s) or _mojibake(s):
        return True
    cjk = len(CJK_RE.findall(s))
    if cjk == 0:
        return True
    if cjk / len(s) > 0.95 and len(s) > 8:
        return True
    return False

def extract(text: str) -> list[dict]:
    """从上下文里抽候选待办（标题 + 可选日期）。"""
    items = []
    seen = set()
    for line in text.splitlines():
        if not line.strip().startswith("ctx:"):
            continue
        chunk = line.split("ctx:", 1)[1]
        for title in TITLE_RE.findall(chunk):
            title = title.strip(" ·-_")
            if len(title) < 4 or title in seen:
                continue
            if any(bad in title for bad in ("IDP_", "SKIN_", "https", "devops", "cpp")):
                continue
            if cjk_ratio(title) < 0.5:
                continue
            seen.add(title)
            near = DATE_RE.search(chunk)
            if _looks_noise(title):
                continue
            items.append({
                "id": "wx:" + str(abs(hash(title)) % (10 ** 10)),
                "title": title,
                "due": near.group(0) if near else "",
                "state": "",
                "source": "wxwork-memory",
            })
    return items


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--rounds", type=int, default=1)
    ap.add_argument("--interval", type=int, default=30)
    args = ap.parse_args()

    if not os.path.isfile(MEMFIND):
        print("MEMFIND_MISSING 先编译 research/wxwork_memfind.cs")
        return 2

    for attempt in range(1, args.rounds + 1):
        if not logged_in():
            print("NOT_LOGGED_IN 企微停在登录/验证页，等扫码后再取证")
            return 3
        pid = wxwork_pid()
        if not pid:
            print(f"round {attempt}: NO_WXWORK（未登录或已退出）")
            if attempt < args.rounds:
                dt_time_sleep(args.interval)
            continue
        found = []
        for marker in MARKERS:
            out = scan_marker(pid, marker)
            if "HIT#" in out:
                found.extend(extract(out))
        if found:
            uniq = {i["title"]: i for i in found}
            payload = {
                "at": dt.datetime.now().strftime("%Y-%m-%d %H:%M"),
                "source": "wxwork 进程内存（只读）",
                "count": len(uniq),
                "items": list(uniq.values()),
            }
            with open(OUT, "w", encoding="utf-8") as fh:
                json.dump(payload, fh, ensure_ascii=False, indent=1)
            print(f"TODO OK items={len(uniq)} -> {OUT}")
            for it in payload["items"][:5]:
                print(f"  {it['title'][:30]}  {it['due']}")
            return 0
        print(f"round {attempt}: 无待办负载（pid={pid}）")
        if attempt < args.rounds:
            dt_time_sleep(args.interval)

    print("TODO_NONE 未捕捉到待办负载（登录后待办面板需被客户端拉取过一次）")
    return 1


def dt_time_sleep(seconds: int) -> None:
    import time
    time.sleep(seconds)


if __name__ == "__main__":
    raise SystemExit(main())
