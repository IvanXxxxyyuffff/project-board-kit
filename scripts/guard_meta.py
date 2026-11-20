#!/usr/bin/env python3
"""仓库门禁：占位化 + 结构 + 一致性。CI 与本机都跑这一个脚本。

退出码：0 通过 / 1 有硬问题（阻断合并）
用法：python scripts/guard_meta.py [--repo <dir>]
"""
import argparse
import hashlib


def norm_bytes(b):
    if _policy is not None:
        if b[:2] == b'MZ':
            return _policy.norm_bytes(b, 'x.exe')
        if b[:4] == b'PK\x03\x04':
            return _policy.norm_bytes(b, 'x.whl')
        return _policy.norm_bytes(b, 'x.txt')
    return b
import json
import os
import re
import sys
sys.dont_write_bytecode = True   # 别把 __pycache__ 写进导出仓（E08 会判脏）

try:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import policy as _policy  # 唯一口径：文本去 BOM/CRLF 归一，二进制原样
except Exception:  # noqa: BLE001
    _policy = None

FAIL = []
MISS_BYTES = []
WARN = []

# ---------- 1. 占位化：具体值形状 ----------
PATH_LEAK = re.compile(r"[A-Za-z]:\\{1,2}(?:Users|[^\\\s]{0,20}[\u4e00-\u9fa5])|[A-Za-z]:\\x\\")
PATH_ANY = re.compile(r"(?<![A-Za-z])[A-Za-z]:\\{1,2}\w")
PATH_OK = re.compile(r"C:\\{1,2}(?:Windows|Program Files|ProgramData)|%ProgramFiles%|%SystemDrive%|\$env:", re.I)
SENSITIVE = [
    ("业务名/客户名", re.compile("0" + "89|Pa" + "cific|Fl" + "avour\\s*Beast|Furu?idon|\u798f\u745e\u987f|Dr" + "ip'?n|\u7535\u5b50\u70df", re.I)),
    ("人名", re.compile("\u6e38\u519b|\u738b\u79b9|\u8c22\u5b87\u5cf0")),
    ("内网地址", re.compile(r"\b(?:(?:10|127)\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b")),
    ("Windows 绝对路径", re.compile(r"(?<![A-Za-z0-9_])[A-Za-z]:\\\\(?:[A-Za-z0-9_.\-]+\\\\)*[A-Za-z0-9_.\-]*")),
    ("凭据字段", re.compile(r"access_token\s*[=:]\s*['\"]?[A-Za-z0-9._\-]{16,}|api[_-]?key\s*[=:]|secret\s*[=:]\s*['\"]?[A-Za-z0-9]{12,}", re.I)),
    ("Cookie 原文", re.compile(r"Cookie:\s*\w+=[^;\s]{12,}", re.I)),
    ("邮箱", re.compile(r"\b[\w.+-]+@[\w-]+\.[\w.]{2,}\b")),
    ("Peer 标识", re.compile(r"Peer\s+169|--peer\s+169\b")),
]

# 允许出现的占位/说明性文本
ALLOW = [
    "<LAN_HOST>", "<ROOT>", "<TOOLS>", "<HOME>", "<CORP_ID>", "<PAD_ID>", "<UID>",
    "<GITHUB_USER>", "<WXWORK_VER>", "<YYNOTE_DIR>", "<TEMP_BUILD>", "<uid>",
    "PROJECT_ROOT", "Program Files", "Users\\", "YOUR_NAME", "<ROOT>",
    "127.0.0.1", "0.0.0.0", "localhost", "<YOUR_KEY>", "示例", "例如",
]
SKIP_FILE = re.compile(r"(package-lock|pnpm-lock|Cargo\.lock|\.min\.js|vendor[\\/]|node_modules[\\/]|scripts[\\/]guard_meta\.py)", re.I)
TOKEN_LIKE_RE = re.compile(r"access_token=[A-Za-z0-9\-_]{16,}|令牌=[A-Za-z0-9\-_]{16,}")


SKIP_LINE = re.compile(r"sha256|checksum|integrity|manifest\.json.*bytes", re.I)
# 内部树独有、按分发策略不进公开套件的文件：不参与「真源 ↔ kit 副本」一致性比对。
# wxhandle.rs 是「独占句柄读取」实验代码，套件里只放一个编译用的空壳（见 _export_build.py 的 STUB_WXHANDLE）。
INTERNAL_ONLY = {"wxhandle.rs"}

# ---------- 2. 结构：必备文件 ----------
REQUIRED = [
    "README.md",
    "AGENTS.md",
    "CHANGELOG.md",
    "docs/20-检测治具.md",
    "docs/30-更新说明.md",
    "docs/40-坑清单.md",
    ".github/workflows/guard.yml",
    "scripts/guard_meta.py",
    "skills/project-board-kit/SKILL.md",
    "skills/project-board-kit/setup.ps1",
    "skills/project-board-kit/discover.ps1",
    "skills/project-board-kit/doctor.ps1",
    "skills/project-board-kit/files/manifest.json",
]


def walk_files(repo):
    for base, dirs, names in os.walk(repo):
        dirs[:] = [d for d in dirs if d not in (".git", "node_modules", "__pycache__")]
        for n in sorted(names):
            yield os.path.join(base, n)


BANNED = [
    # (正则, 说明, 限定扩展名 or None=全部)
    (r"\b" + "0" + "89" + r"\b", "客户代号", None),
    ("Dr" + "ip'?n|" + "Dr" + "ippin", "品牌名", None),
    ("AC" + "ME_MFG", "公司名", None),
    (r"\u6e38\u519b", "人名", None),
    (r"Peer\s*169", "Peer 标识", None),
    (r"MODEL_0\d{2}", "机型占位", None),
    (r"access_token\s*[=:]\s*[\-A-Za-z0-9]{12,}", "凭据字面量", None),
    (("NOTE" + "123"), "口令样例", None),
    (r"\b244\b|\b370\b", "内部编号", (".md", ".json", ".txt")),
]


def check_business_terms(repo):
    """业务/客户/人名/机型残留回潮检查（CI 与本地同一脚本）。"""
    hits = 0
    for rel in walk_files(repo):
        if SKIP_FILE.search(rel):
            continue
        ext = os.path.splitext(rel)[1].lower()
        p = os.path.join(repo, rel)
        try:
            txt = norm_bytes(open(p, "rb").read()).decode("utf-8", "ignore")
        except OSError:
            continue
        for rx, why, exts in BANNED:
            if exts and ext not in exts:
                continue
            m = re.search(rx, txt)
            if m:
                line = txt[: m.start()].count("\n") + 1
                FAIL.append(f"FAIL [业务残留/{why}] {rel}:{line} -> {m.group(0)[:40]!r}")
                hits += 1
    return hits


PLACEHOLDER = "<" + "ROOT>"
BIN_EXT = {'.whl', '.dll', '.exe', '.zip', '.png', '.jpg', '.jpeg', '.ico', '.pdf', '.pyd', '.so'}


ABS_PATH = re.compile(r"[A-Za-z]:\\{1,2}[^\\\s'\"<>|*?]{1,40}\\[A-Za-z0-9A-Za-z0-9_.\-\u4e00-\u9fa5]|[A-Za-z]:/(?:Users|Windows|ProgramData|Tools|tools|Temp|tmp)/")


def check_placeholder_integrity(repo):
    """清单完整性 + 占位符完整性：防止外部批量改写破坏 hash 或把占位符写成绝对路径。"""
    kit = os.path.join(repo, "skills/project-board-kit")
    mani = os.path.join(kit, "files/manifest.json")
    if not os.path.isfile(mani):
        FAIL.append("[占位符] 缺少 files/manifest.json")
        return
    try:
        m = json.loads(norm_bytes(open(mani, "rb").read()).decode("utf-8"))
    except Exception as e:  # noqa: BLE001
        FAIL.append(f"[占位符] manifest.json 解析失败：{e}")
        return
    entries = m.get("files") or {}
    missing, bad, leaked = 0, 0, 0
    for rel, meta in entries.items():
        p = os.path.join(kit, rel)
        if not os.path.isfile(p):
            FAIL.append(f"[占位符] 清单声明的文件不存在：{rel}")
            missing += 1
            continue
        data = norm_bytes(open(p, "rb").read())
        if len(data) != meta.get("bytes"):
            FAIL.append(f"[占位符] 字节数与清单不符：{rel} ({len(data)} != {meta.get('bytes')})")
            bad += 1
        if hashlib.sha256(data).hexdigest() != meta.get("sha256"):
            FAIL.append(f"[占位符] sha256 与清单不符：{rel}")
            bad += 1
        if os.path.splitext(rel)[1].lower() in BIN_EXT:
            continue  # 二进制产物不做文本扫描（wheel 内含随机字节会被误判）
        lines = [ln for ln in data.decode("utf-8", "ignore").splitlines() if "://" not in ln]
        if ABS_PATH.search("\n".join(lines)):
            FAIL.append(f"[占位符] 出现盘符绝对路径（占位符被写死）：{rel}")
            leaked += 1
    has_ph = False
    for rel in entries:
        p = os.path.join(kit, rel)
        if os.path.isfile(p) and PLACEHOLDER in open(p, encoding="utf-8", errors="ignore").read():
            has_ph = True
            break
    if entries and not has_ph:
        FAIL.append("[占位符] 清单文件里已找不到 <ROOT> 占位符，占位机制被破坏")
    print(f"PLACEHOLDER files={len(entries)} missing={missing} bad={bad} leaked={leaked} has_ph={has_ph}")


def check_sensitive(repo):
    scanned = 0
    for p in walk_files(repo):
        rel = os.path.relpath(p, repo).replace("\\", "/")
        if SKIP_FILE.search(rel) or rel.endswith((".png", ".jpg", ".zip", ".exe", ".dll", ".pdf")):
            continue
        try:
            text = open(p, encoding="utf-8", errors="ignore").read()
        except OSError:
            continue
        scanned += 1
        for i, line in enumerate(text.splitlines(), 1):
            if SKIP_LINE.search(line):
                continue
            probe = line
            for a in ALLOW:
                probe = probe.replace(a, "")
            for name, rx in SENSITIVE:
                m = rx.search(probe)
                if m:
                    FAIL.append(f"[{name}] {rel}:{i} -> {m.group(0)[:60]!r}")
            if "<" not in line:
                m2 = PATH_LEAK.search(line)
                if m2:
                    FAIL.append(f"[路径残留] {rel}:{i} -> {m2.group(0)[:60]!r}")
                else:
                    for tok in PATH_ANY.findall(line):
                        if PATH_OK.search(tok):
                            continue
                        FAIL.append(f"[盘符路径] {rel}:{i} -> {tok[:40]!r}")
                        break
    return scanned


def check_required(repo):
    for rel in REQUIRED:
        if not os.path.isfile(os.path.join(repo, rel)):
            FAIL.append(f"[缺失必备文件] {rel}")


def check_tokens(repo):
    hits = []
    for p in walk_files(repo):
        rel = os.path.relpath(p, repo).replace("\\", "/")
        if SKIP_FILE.search(rel) or rel.endswith((".lock", ".lockb")):
            continue
        try:
            text = open(p, encoding="utf-8", errors="ignore").read()
        except Exception:
            continue
        for ln, line in enumerate(text.splitlines(), 1):
            if TOKEN_LIKE_RE.search(line):
                hits.append(f"{rel}:{ln}")
    if hits:
        FAIL.append(f"[令牌形状] {len(hits)} 处，例：{hits[:3]}")


def check_manifest(repo):
    mp = os.path.join(repo, "skills/project-board-kit/files/manifest.json")
    if not os.path.isfile(mp):
        return
    try:
        doc = json.load(open(mp, encoding="utf-8"))
    except Exception:
        FAIL.append("[manifest 无法解析] skills/project-board-kit/files/manifest.json")
        return
    files = doc.get("files") or {}
    bad = [k for k, v in files.items() if not isinstance(v.get("bytes"), int)]
    if bad:
        FAIL.append(f"[manifest bytes 非整数] {len(bad)} 项，例：{bad[:3]}")
    bad_sha = [k for k, v in files.items() if not re.fullmatch(r"[0-9a-f]{64}", str(v.get("sha256")))]
    if bad_sha:
        FAIL.append(f"[manifest sha256 非法] {len(bad_sha)} 项，例：{bad_sha[:3]}")
    kit = os.path.join(repo, "skills/project-board-kit")
    missing = [k for k in files if not os.path.isfile(os.path.join(kit, k))]
    if missing:
        FAIL.append(f"[清单登记但实体缺失] {len(missing)} 项，例：{missing[:5]}")
    extra = []
    froot = os.path.join(kit, "files")
    for p in walk_files(froot):
        rel = os.path.relpath(p, kit).replace("\\", "/")
        if rel.endswith("manifest.json"):
            continue
        if rel not in files:
            extra.append(rel)
    if extra:
        FAIL.append(f"[files/ 有文件但清单未登记] {len(extra)} 项，例：{extra[:5]}")
    mism = []
    for k, v in files.items():
        p = os.path.join(kit, k)
        if not os.path.isfile(p):
            continue
        blob = norm_bytes(open(p, "rb").read())  # 去 BOM + 归一换行
        if len(blob) != v.get("bytes") or hashlib.sha256(blob).hexdigest() != str(v.get("sha256")):
            mism.append(k)
    if mism:
        FAIL.append(f"[manifest 与实体不符] {len(mism)} 项，例：{mism[:5]}")


def check_installer_order(repo):
    sk = os.path.join(repo, "skills/project-board-kit/SKILL.md")
    if not os.path.isfile(sk):
        return
    text = open(sk, encoding="utf-8", errors="ignore").read()
    i_setup = text.find("setup.ps1")
    i_disc = text.find("discover.ps1")
    i_doc = text.find("doctor.ps1")
    has_order = ("setup.ps1` → `discover.ps1` → `doctor.ps1`" in text
                 or "setup.ps1 → discover.ps1 → doctor.ps1" in text)
    if not has_order:
        FAIL.append("[安装顺序] SKILL.md 缺少显式顺序：setup.ps1 → discover.ps1 → doctor.ps1")
    if min(i_setup, i_disc, i_doc) < 0:
        FAIL.append("[安装顺序] SKILL.md 未同时提到 setup/discover/doctor")


def check_exe_policy(repo):
    kit = os.path.join(repo, "skills/project-board-kit")
    exe = os.path.join(kit, "files/bin/项目看板.exe")
    if not os.path.isfile(exe):
        readme = os.path.join(repo, "README.md")
        skill = os.path.join(kit, "SKILL.md")
        ok = False
        for p in (readme, skill):
            if os.path.isfile(p):
                t = open(p, encoding="utf-8", errors="ignore").read()
                if "构建" in t or "build" in t.lower() or "Release" in t:
                    ok = True
        if not ok:
            FAIL.append("[exe 交付] 套件未含 exe，且文档未说明如何获取/构建")


# 交付包一致性：kit 里的 Rust 源码/构建文件必须与仓库真源逐字节一致，
# 以及会议链路的两个脚本也要一致。否则「按文档自建」得到的是旧二进制。
PARITY_GROUPS = [
    ("rust-widget/src", "skills/project-board-kit/files/rust-widget/src", (".rs",)),
    ("rust-widget", "skills/project-board-kit/files/rust-widget", ("Cargo.toml", "Cargo.lock", "build.rs")),
    ("rust-widget", "skills/project-board-kit/files/rust-widget", ("README.md",)),
    # ("**",) 表示递归比对两个目录下所有同名文本文件，避免清单外的文件悄然漂移
    ("research", "skills/project-board-kit/files/research", ("**",)),
]

PARITY_EXT = (".py", ".mjs", ".js", ".ps1", ".md", ".json", ".cs", ".rs", ".toml", ".yml", ".yaml", ".html", ".css")
PARITY_SKIP = ("raw/", "wxdata/", "api/", "_raw/", "_legacy/", "_rv/", "_t/", "__pycache__/", "node_modules/")


def check_kit_parity(repo):
    bad, checked = [], 0
    for rel_a, rel_b, names in PARITY_GROUPS:
        da, db = os.path.join(repo, rel_a), os.path.join(repo, rel_b)
        if names == ("**",):
            if not os.path.isdir(da):
                continue
            todo = []
            for root, dirs, fs in os.walk(da):
                dirs[:] = [d for d in dirs if d not in ("__pycache__", "node_modules", ".git", "target")]
                for f in fs:
                    if not f.lower().endswith(PARITY_EXT):
                        continue
                    rel = os.path.relpath(os.path.join(root, f), da).replace("\\", "/")
                    if any(seg in rel for seg in PARITY_SKIP):
                        continue
                    todo.append(rel)
            for rel in sorted(todo):
                pa = os.path.join(da, rel.replace("/", os.sep))
                pb = os.path.join(db, rel.replace("/", os.sep))
                if not os.path.exists(pb):
                    bad.append((os.path.join(rel_b, rel), "kit 缺该文件"))
                    continue
                ha = hashlib.sha256(norm_bytes(open(pa, "rb").read())).hexdigest()
                hb = hashlib.sha256(norm_bytes(open(pb, "rb").read())).hexdigest()
                checked += 1
                if ha != hb:
                    bad.append((os.path.join(rel_b, rel), "与真源不一致"))
            continue
        for name in names:
            if name.startswith("."):
                if not os.path.isdir(da) or not os.path.isdir(db):
                    continue
                files = sorted(f for f in os.listdir(da) if f.endswith(name))
            else:
                files = [name]
            for f in files:
                if f in INTERNAL_ONLY:
                    continue
                pa, pb = os.path.join(da, f), os.path.join(db, f)
                if not os.path.exists(pa):
                    continue
                if not os.path.exists(pb):
                    bad.append((os.path.join(rel_b, f), "kit 缺该文件"))
                    continue
                ha = hashlib.sha256(norm_bytes(open(pa, "rb").read())).hexdigest()
                hb = hashlib.sha256(norm_bytes(open(pb, "rb").read())).hexdigest()
                checked += 1
                if ha != hb:
                    bad.append((os.path.join(rel_b, f), "与真源不一致"))
    for rel, why in bad[:6]:
        FAIL.append(f"[副本滞后] {rel} -> {why}")
    print("PARITY checked=%d bad=%d" % (checked, len(bad)))
    return not bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    a = ap.parse_args()
    repo = os.path.abspath(a.repo)

    scanned = check_sensitive(repo)
    check_business_terms(repo)
    check_placeholder_integrity(repo)
    check_kit_parity(repo)
    check_required(repo)
    check_tokens(repo)
    check_manifest(repo)
    check_installer_order(repo)
    check_exe_policy(repo)

    for w in WARN:
        print("WARN", w)
    for f in FAIL[:40]:
        print("FAIL", f)
    total = len(FAIL)
    print(f"GUARD scanned={scanned} fails={total} warns={len(WARN)}")
    print("GUARD_RESULT=" + ("PASS" if total == 0 else "FAIL"))
    return 0 if total == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
