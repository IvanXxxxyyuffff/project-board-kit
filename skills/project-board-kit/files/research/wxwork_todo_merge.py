"""把企微待办并入挂件「部门进度」待办列表（幂等、可重复跑）。

输入：
    research/wxwork_todo.json     # wxwork_todo_capture.py 的产物，{items:[{id,title,due}]}
    <TOOLS>/项目看板/widget_state.json 或 --state 指定的 state

规则：
    - 只增不删：已存在的同 id / 同标题条目跳过
    - 企微条目 id 前缀 `wxtodo_`，due 为空时写今天（与手填待办同口径）
    - 先备份 state 到 `<state>.bak-wxtodo`
    - 与 yynote 导入相同：不覆盖用户手改过的文本，只补缺字段

用法：
    python wxwork_todo_merge.py                       # 默认路径
    python wxwork_todo_merge.py --state X.json --dry-run
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import os
import shutil

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SRC = os.path.join(HERE, "wxwork_todo.json")
DEFAULT_STATE = r"<TOOLS>\项目看板\widget_state.json"


def norm(text: str) -> str:
    return " ".join((text or "").split()).strip().lower()


def load_json(path: str, default):
    try:
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except Exception:
        return default


def merge(src_path: str, state_path: str, dry_run: bool, due_default: str) -> int:
    src = load_json(src_path, {})
    items = src.get("items") or []
    if not items:
        print("MERGE_WX skipsrc=empty")
        return 0

    state = load_json(state_path, None)
    if state is None:
        print(f"MERGE_WX state_missing={state_path}")
        return 2

    dept = state.setdefault("department", [])
    have_ids = {str(it.get("id", "")) for it in dept}
    have_text = {norm(it.get("text", "")) for it in dept}
    arch_ids = {str(a.get("id", "")) for a in state.get("archive", [])}

    added = 0
    for it in items:
        if _looks_noise(it.get('title')):
            continue
        if _noise(it.get('title')):
            continue
        title = (it.get("title") or "").strip()
        if len(title) < 2:
            continue
        wid = "wxtodo_" + str(it.get("id") or abs(hash(title)) % (10 ** 8))
        if wid in have_ids or wid in arch_ids:
            continue
        if norm(title) in have_text:
            continue
        row = {
            "id": wid,
            "text": title,
            "done": False,
            "due": (it.get("due") or "").strip() or due_default,
            "src": "wxwork",
        }
        dept.append(row)
        have_ids.add(wid)
        have_text.add(norm(title))
        added += 1

    if added and not dry_run:
        shutil.copyfile(state_path, state_path + ".bak-wxtodo")
        tmp = state_path + ".tmp"
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(state, fh, ensure_ascii=False, indent=1)
        os.replace(tmp, state_path)

    print(f"MERGE_WX items={len(items)} added={added} dept={len(dept)} dry={'1' if dry_run else '0'}")
    return 0



def _noise(title):
    t = (title or "").strip()
    if len(t) < 2 or len(t) > 60:
        return True
    if re.search(r"[\ue000-\uf8ff]", t):
        return True
    try:
        raw = t.encode("utf-8", errors="ignore")
        if len(raw) % 2 == 0:
            back = raw.decode("utf-16-le", errors="ignore")
            if back and sum(1 for c in back if 0x20 <= ord(c) < 0x7F) / len(back) > 0.8:
                return True
    except Exception:
        pass
    # 纯数字/字母的标题（机型号、编号）也是合法待办，只有「既无中文也无字母数字」才判噪声
    return re.search(r"[0-9A-Za-z\u4e00-\u9fff]", t) is None



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
    # 纯数字/字母标题（机型号、编号、临时记录）也是合法待办，只有「无中文且无字母数字」才判噪声
    if cjk == 0 and re.search(r"[0-9A-Za-z]", s) is None:
        return True
    if cjk / len(s) > 0.95 and len(s) > 8:
        return True
    return False

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", default=DEFAULT_SRC)
    ap.add_argument("--state", default=DEFAULT_STATE)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if not os.path.isfile(args.src):
        print(f"MERGE_WX skipsrc=missing:{args.src}")
        return 0
    due_default = dt.date.today().isoformat()
    return merge(args.src, args.state, args.dry_run, due_default)


if __name__ == "__main__":
    raise SystemExit(main())
