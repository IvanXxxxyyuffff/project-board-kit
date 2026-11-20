#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
publish gate — 发布前全面检查（仓库内可运行版）
=====================================
目的：把「推送到云端之后才发现的问题」全部前置到推送前，用一条命令给出可复核的清单。
它检查的是 **将要被推送的那棵干净树**（默认 <ROOT>\\_export\\repo），而不是业务工作树。

设计原则
--------
1. 单一入口：`prepublish_gate.py [--repo DIR] [--version vX.Y] [--selftest]`，退出码 0=全绿。
2. 每项检查有稳定编号（E01…E21），失败信息里带文件与行号，可直接定位。
3. 真实业务词表**不放在仓库里**（放在 `_business/leak_words.txt`），避免「用敏感词去查敏感词」导致词表自己泄漏。
4. `--selftest` 会复制一份干净树、逐项注入违规，断言对应检查确实变红 —— 防止「治具没说谎，但范围不全」。
5. 只读：除 `--selftest` 产生的临时目录外不写任何文件。

用法
----
    python -X utf8 _business/prepublish_gate.py --version v2.11
    python -X utf8 _business/prepublish_gate.py --selftest        # 自检覆盖度
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import ast
import re
import sys

# 必须在 import 仓库内模块之前：本检查会扫 __pycache__/.pyc，而 import policy 自己就会写 pyc，
# 不关掉字节码就会出现「检查器把自己的缓存判成仓库污染」的假红（复核 2026-11-19 实测）。
sys.dont_write_bytecode = True

try:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import policy as _policy
except Exception:  # noqa: BLE001
    _policy = None
import shutil
import subprocess
import tempfile
import uuid

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_REPO = os.getcwd()
LEAK_FILE = os.environ.get("PREPUB_LEAK_FILE") or os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "leak_words.txt")

TEXT_EXT = (".py", ".mjs", ".js", ".ps1", ".md", ".json", ".cs", ".rs", ".toml",
            ".yml", ".yaml", ".html", ".css", ".txt", ".gitignore", ".gitattributes", ".example")
BIN_ALLOW = {
    "skills/project-board-kit/files/vendor/libcrypto32.dll",        # 企微库解密所需 32 位 OpenSSL
}
BIN_ALLOW_PREFIX = (
    "skills/project-board-kit/files/vendor/wheels/",                # 离线依赖包（cp312 wheel）
)
JUNK_NAME = re.compile(
    r"(^|/)(_gout\.txt|out\.txt|nohup\.out|stderr\.txt|.*\.log|.*\.bak|.*\.bak-\d+|.*\.tmp|"
    r"__pycache__|node_modules|\.finesse|\.idea|\.vscode|\.DS_Store|Thumbs\.db)(/|$)", re.I)
JUNK_DIR = re.compile(r"(^|/)(_v\d+|_vfy\d*|_rk\d*|_rd\d*|_export|_business|_buildkit|_raw|target|dist|build)(/|$)")
DRIVE_PATH = re.compile(r"(?<![A-Za-z])[A-Za-z]:[\\/]{1,2}[A-Za-z0-9_.\-\u4e00-\u9fa5]")
SAFE_PATH = re.compile(r"^(?:[Cc]:[\\/](?:Windows|Program Files|ProgramData|Users[\\/]Public)|%ProgramFiles%|%SystemDrive%|\$env:|<)")
HEADING = re.compile(r"^#{1,3}\s+(.+?)\s*$", re.M)
VERSION_HEAD = re.compile(r"^##\s+v?(\d+\.\d+(?:\.\d+)?)", re.M)


class Result:
    def __init__(self, code: str, title: str):
        self.code, self.title = code, title
        self.bad: list[str] = []
        self.note = ""

    @property
    def ok(self) -> bool:
        return not self.bad

    def fail(self, msg: str) -> None:
        self.bad.append(msg)


def walk(repo: str):
    for root, dirs, files in os.walk(repo):
        dirs[:] = [d for d in dirs if d != ".git"]
        for f in files:
            p = os.path.join(root, f)
            yield os.path.relpath(p, repo).replace("\\", "/"), p


def read_text(p: str) -> str:
    try:
        with open(p, "rb") as fh:
            raw = fh.read()
        if b"\x00" in raw[:4096]:
            return ""
        return raw.decode("utf-8", "ignore")
    except OSError:
        return ""


def norm_bytes(raw: bytes) -> bytes:
    if raw.startswith(b"\xef\xbb\xbf"):
        raw = raw[3:]
    return raw.replace(b"\r\n", b"\n")


# ---------------------------------------------------------------- 检查项

def e01_guard(repo: str) -> Result:
    r = Result("E01", "内建门禁 guard_meta.py 全绿（脱敏/清单/占位符/一致性/安装顺序/exe 策略）")
    script = os.path.join(repo, "scripts", "guard_meta.py")
    if not os.path.exists(script):
        r.fail("缺 scripts/guard_meta.py")
        return r
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
    cp = subprocess.run([sys.executable, "-X", "utf8", "-B", script], cwd=repo, env=env,
                        capture_output=True, text=True, encoding="utf-8", errors="ignore")
    out = (cp.stdout or "") + (cp.stderr or "")
    r.note = next((ln.strip() for ln in out.splitlines() if "GUARD scanned" in ln), "")
    if "GUARD_RESULT=PASS" not in out:
        for ln in out.splitlines():
            if ln.strip().startswith("FAIL"):
                r.fail(ln.strip()[:200])
        if not r.bad:
            r.fail("guard 未输出 PASS：" + out.strip().splitlines()[-1][:160] if out.strip() else "guard 无输出")
    return r


def e02_leak_words(repo: str) -> Result:
    r = Result("E02", "真实业务词表零命中（词表在仓库外，避免自查即泄漏）")
    lf = os.environ.get("PREPUB_LEAK_FILE") or LEAK_FILE
    if not os.path.exists(lf):
        # 词表在仓库外（公开仓不能自带），但缺词表就等于这一项恒空转：
        # 发布时静默 PASS 会让人误以为"扫过了"。默认按失败处理，
        # 只有本地调试可以显式 PREPUB_ALLOW_NO_WORDS=1 放行。
        if os.environ.get("PREPUB_ALLOW_NO_WORDS") == "1":
            r.note = "跳过：未提供词表（PREPUB_ALLOW_NO_WORDS=1，仅限本地调试）"
            return r
        r.fail("未提供词表（%s）：无法验证真实业务词零命中；本地调试可设 PREPUB_ALLOW_NO_WORDS=1" % lf)
        return r
    words = [w.strip() for w in read_text(lf).splitlines() if w.strip() and not w.startswith("#")]
    pats = []
    for w in words:
        if re.fullmatch(r"[\x20-\x7e]+", w):          # 纯 ASCII：加词边界，避免 "CLIENT_A" 命中 ellipsis
            pats.append((w, re.compile(r"(?<![A-Za-z0-9])" + re.escape(w) + r"(?![A-Za-z0-9])", re.I)))
        else:
            pats.append((w, re.compile(re.escape(w), re.I)))
    r.note = f"词表 {len(words)} 条"
    for rel, p in walk(repo):
        if not rel.lower().endswith(TEXT_EXT):
            continue
        for i, line in enumerate(read_text(p).splitlines(), 1):
            for w, pat in pats:
                if pat.search(line):
                    r.fail(f"{rel}:{i} 命中「{w}」-> {line.strip()[:80]}")
    return r


def e03_abs_paths(repo: str) -> Result:
    r = Result("E03", "无本机绝对路径（盘符/用户目录/临时目录）")
    n = 0
    for rel, p in walk(repo):
        if not rel.lower().endswith(TEXT_EXT):
            continue
        for i, line in enumerate(read_text(p).splitlines(), 1):
            if "://" in line:
                line = re.sub(r"\w+://\S+", "", line)
            for m in DRIVE_PATH.finditer(line):
                n += 1
                if SAFE_PATH.search(m.group(0)):
                    continue
                r.fail(f"{rel}:{i} -> {m.group(0)[:60]}")
    r.note = f"扫描到 {n} 处盘符形态"
    return r


def e04_placeholder(repo: str) -> Result:
    r = Result("E04", "安装路径占位符 `<ROOT>` 存活且清单可还原")
    man = os.path.join(repo, "skills", "project-board-kit", "files", "manifest.json")
    if not os.path.exists(man):
        r.fail("缺 manifest.json")
        return r
    txt = read_text(man)
    try:
        data = json.loads(txt)
    except Exception as exc:                                   # noqa: BLE001
        r.fail(f"manifest.json 无法解析：{exc}")
        return r
    files = data.get("files") or {}
    r.note = f"清单 {len(files)} 项"
    if not files:
        r.fail("manifest.files 为空")
        return r
    bad_int = [k for k, v in files.items() if not isinstance(v.get("bytes"), int)]
    if bad_int:
        r.fail(f"{len(bad_int)} 项 bytes 不是整数（脱敏误改数字的老问题）：{bad_int[:3]}")
    bad_sha = [k for k, v in files.items() if not re.fullmatch(r"[0-9a-f]{64}", str(v.get("sha256", "")))]
    if bad_sha:
        r.fail(f"{len(bad_sha)} 项 sha256 形态异常：{bad_sha[:3]}")
    # 实体 ↔ 声明 双向一致 + 字节数（归一化后）
    kit_root = "skills/project-board-kit"
    base = os.path.join(repo, kit_root, "files")
    declared = set()
    for k in files:
        kk = k.replace("\\", "/")
        declared.add(kk.split("files/", 1)[1] if kk.startswith("files/") else kk)
    for rel, meta in files.items():
        kk = rel.replace("\\", "/")
        inner = kk.split("files/", 1)[1] if kk.startswith("files/") else kk
        p = os.path.join(base, inner.replace("/", os.sep))
        if not os.path.exists(p):
            r.fail(f"清单声明但实体缺失：{inner}")
            continue
        with open(p, "rb") as fh:
            _raw = fh.read()
            # 二进制（PE/zip/wheel）按原始字节，不做 BOM/CRLF 归一
            nb = _raw if (_raw[:2] == b'MZ' or _raw[:4] == b'PK\x03\x04' or b'\x00' in _raw[:512]) else norm_bytes(_raw)
        if len(nb) != meta.get("bytes"):
            r.fail(f"字节数不符：{inner} 声明 {meta.get('bytes')} 实为 {len(nb)}")
        if hashlib.sha256(nb).hexdigest() != meta.get("sha256"):
            r.fail(f"sha256 不符：{inner}")
    for root, _dirs, fs in os.walk(base):
        for f in fs:
            rel = os.path.relpath(os.path.join(root, f), base).replace("\\", "/")
            if rel not in declared and rel != "manifest.json":
                r.fail(f"实体未入清单：{rel}")
    # 占位符机制：kit 里必须还有文件带 `<ROOT>`（doctor/discover 的还原逻辑依赖它）
    ph_files = []
    for rel, p in walk(os.path.join(repo, kit_root)):
        if rel.lower().endswith(TEXT_EXT) and "<ROOT>" in read_text(p):
            ph_files.append(rel)
    r.note += f"，含 <ROOT> 的文件 {len(ph_files)} 个"
    if not ph_files:
        r.fail("kit 内已无任何 `<ROOT>` 占位符（安装路径无法还原）")
    return r


def e05_docs(repo: str) -> Result:
    r = Result("E05", "必备文档齐全且无重复章节")
    need = ["README.md", "AGENTS.md", "CHANGELOG.md", "docs/00-维护手册.md", "docs/10-模块索引.md",
            "docs/20-检测治具.md", "docs/30-更新说明.md", "docs/40-坑清单.md", ".gitattributes"]
    for n in need:
        if not os.path.exists(os.path.join(repo, n.replace("/", os.sep))):
            r.fail(f"缺 {n}")
    dup_total = 0
    for rel, p in walk(repo):
        if not rel.lower().endswith(".md"):
            continue
        # CHANGELOG 是多版本日志，「改了什么 / 为什么改」天然会重复，不参与查重
        if os.path.basename(p).lower() == 'changelog.md':
            continue
        seen, dup = {}, 0
        for m in HEADING.finditer(read_text(p)):
            key = m.group(1).strip()
            seen[key] = seen.get(key, 0) + 1
            if seen[key] == 2:
                dup += 1
                r.fail(f"{rel} 重复章节标题：「{key}」")
        dup_total += dup
    r.note = f"md 文件 {sum(1 for rel, _ in walk(repo) if rel.lower().endswith('.md'))} 个，重复标题 {dup_total}"
    ga = read_text(os.path.join(repo, ".gitattributes"))
    if "-text" not in ga:
        r.fail(".gitattributes 缺 `* -text`（换行转换会让清单字节数失真）")
    return r


def _vkey(v: str):
    parts = [int(x) for x in re.findall(r"\d+", v)]
    while len(parts) > 1 and parts[-1] == 0:
        parts.pop()
    return tuple(parts)


def e06_version(repo: str, version: str | None) -> Result:
    r = Result("E06", "版本号一致（CHANGELOG ↔ Cargo.toml ↔ 发布参数）")
    cl = os.path.join(repo, "CHANGELOG.md")
    if not os.path.exists(cl):
        r.fail("缺 CHANGELOG.md")
        return r
    m1 = VERSION_HEAD.search(read_text(cl))
    if not m1:
        r.fail("CHANGELOG 顶部没有 ## vX.Y 版本行")
        return r
    head = m1.group(1)
    cargo = None
    for cand in (("rust-widget", "Cargo.toml"),
                 ("skills", "project-board-kit", "files", "rust-widget", "Cargo.toml")):
        fp = os.path.join(repo, *cand)
        if os.path.exists(fp):
            cargo = fp
            break
    cv = "?"
    if cargo:
        m2 = re.search(r'^version\s*=\s*"([^"]+)"', read_text(cargo), re.M)
        if m2:
            cv = m2.group(1)
    r.evidence = f"CHANGELOG={head} Cargo={cv}" + (f" 参数={version}" if version else "")
    if not cargo or cv == "?":
        r.fail("找不到 rust-widget/Cargo.toml 的 version，无法核对")
    elif _vkey(cv) != _vkey(head):
        r.fail(f"Cargo.toml 版本 {cv} 与 CHANGELOG 顶部 {head} 不一致（改版本必须三处同步）")
    if version:
        v = version.lstrip("vV")
        if _vkey(v) != _vkey(head):
            r.fail(f"发布参数 {version} 与 CHANGELOG 顶部 {head} 不一致")
    return r


def e07_install_chain(repo: str) -> Result:
    r = Result("E07", "安装链可执行性（setup→discover→doctor 顺序、占位符还原、脚本编译）")
    kit = os.path.join(repo, "skills", "project-board-kit")
    setup = read_text(os.path.join(kit, "setup.ps1"))
    disc = read_text(os.path.join(kit, "discover.ps1"))
    doctor = read_text(os.path.join(kit, "doctor.ps1"))
    skill = read_text(os.path.join(kit, "SKILL.md"))
    for name, txt in (("setup.ps1", setup), ("discover.ps1", disc), ("doctor.ps1", doctor)):
        if not txt:
            r.fail(f"缺 {name}")
    if setup and "$skipNames" not in setup:
        r.fail("setup.ps1 缺统一排除名单 $skipNames（会改写 doctor/discover 自身占位符）")
    if setup:
        for nm in ("doctor.ps1", "discover.ps1"):
            if nm not in setup:
                r.fail(f"setup.ps1 的排除名单未包含 {nm}")
    if doctor and "'<' + 'ROOT>'" not in doctor and "\"<\" + \"ROOT>\"" not in doctor:
        r.fail("doctor.ps1 未用拼接方式生成 `<ROOT>`（会被批量改写而失真）")
    if disc and "SystemDrive" not in disc and "E-Mobile" in disc:
        r.fail("discover.ps1 仍写死 E-Mobile 安装盘符，应为 $env:SystemDrive")
    arrow = r"(?:→|->|=>)"
    if not re.search(r"setup\.ps1[^\n]{0,16}" + arrow + r"[^\n]{0,16}discover\.ps1[^\n]{0,16}" + arrow + r"[^\n]{0,16}doctor", skill):
        r.fail("SKILL.md 缺少「setup → discover → doctor」这一整行顺序说明")
    if re.search(r"discover[^\n]{0,12}" + arrow + r"[^\n]{0,12}doctor[^\n]{0,12}" + arrow + r"[^\n]{0,12}setup\.ps1", skill):
        r.fail("SKILL.md 仍残留错误顺序 discover → doctor → setup")
    if "em_cookie_scan" not in setup:
        r.fail("setup.ps1 未包含 em_cookie_scan 编译步骤（会议链路会永久失效）")
    elif not re.search(r"csc\.exe|Add-Type", setup):
        r.fail("setup.ps1 的 em_cookie_scan 编译无 csc/Add-Type 兜底")
    for nm, txt in (("setup.ps1", setup), ("discover.ps1", disc), ("doctor.ps1", doctor)):
        if not txt:
            continue
        with open(os.path.join(kit, nm), "rb") as fh:
            if not fh.read(3) == b"\xef\xbb\xbf":
                r.fail(f"{nm} 缺 UTF-8 BOM（PowerShell 5.1 会把中文当 ANSI）")
    return r


def _find_csc():
    import glob
    roots = [
        os.path.join(os.environ.get("WINDIR") or os.environ.get("SystemRoot", "") or ".", "Microsoft.NET", "Framework"),
        os.path.join(os.environ.get("WINDIR") or os.environ.get("SystemRoot", "") or ".", "Microsoft.NET", "Framework64"),
    ]
    for root in roots:
        hits = sorted(glob.glob(os.path.join(root, "v4*", "csc.exe")))
        if hits:
            return hits[-1]
    return None


def e12_csharp(repo: str) -> Result:
    """导出的 C# 采集器必须能编译：代码里出现占位符（如把长度常量换成 CLIENT_B）会在这里红。"""
    r = Result("E12", "导出的 C# 采集器可编译（脱敏不破坏代码字面量）")
    csc = _find_csc()
    if not csc:
        r.note = "未找到 csc.exe，跳过编译检查"
        return r
    files = []
    seen_cs = set()
    for dirpath, _dirs, names in os.walk(repo):
        for n in names:
            if n.endswith(".cs") and (os.path.basename(dirpath) == "research"
                                      or os.sep + "research" + os.sep in dirpath):
                # 真源与套件副本同名：只编一份，否则输出 dll 撞名 CS0016
                if n in seen_cs:
                    continue
                seen_cs.add(n)
                files.append(os.path.join(dirpath, n))
    if not files:
        r.note = "仓库内没有 .cs 采集器，跳过"
        return r
    import tempfile
    bad = []
    with tempfile.TemporaryDirectory() as tmp:
        for f in files:
            out = os.path.join(tmp, os.path.basename(f) + ".dll")
            p = subprocess.run([csc, "/nologo", "/target:library", "/out:" + out, f],
                               capture_output=True, text=True, encoding="utf-8", errors="replace")
            if p.returncode != 0:
                first = (p.stdout or p.stderr or "").strip().splitlines()
                bad.append((os.path.relpath(f, repo), first[0] if first else "编译失败"))
    if bad:
        for rel, msg in bad:
            r.fail(f"{rel}: {msg}")
    else:
        r.note = f"{len(files)} 个 .cs 全部编译通过"
    return r


def e08_junk(repo: str) -> Result:
    r = Result("E08", "无备份/缓存残留（扫文件系统，不只看 git）")
    for rel, _p in walk(repo):
        # .pyc/.pyo 是编译缓存：导出仓里出现就是导出链没滤干净（曾带进 4 个）
        if rel.endswith((".pyc", ".pyo")) or "__pycache__/" in rel:
            r.fail(f"编译缓存进了导出仓：{rel}")
            continue
        if JUNK_NAME.search(rel):
            r.fail(f"垃圾文件：{rel}")
        if JUNK_DIR.search(rel):
            r.fail(f"工作目录混入仓库：{rel}")
    return r


def e09_size_binary(repo: str) -> Result:
    r = Result("E09", "无超大文件/未声明二进制")
    big = 0
    for rel, p in walk(repo):
        sz = os.path.getsize(p)
        if sz > 5 * 1024 * 1024:
            r.fail(f"超大文件 {sz // 1024}KB：{rel}")
        low = rel.lower()
        if low.endswith((".exe", ".dll", ".pdb", ".zip", ".7z", ".png", ".jpg", ".ico", ".pdf", ".whl")):
            if rel in BIN_ALLOW or rel.startswith(BIN_ALLOW_PREFIX):
                big += 1          # 只有「确实被声明放行」的才算白名单二进制
            else:
                r.fail(f"未声明二进制：{rel}")
    r.note = f"白名单二进制 {big} 个"
    return r


NONEXPORT_PREFIX = ("research/client_", "research/fc_", "research/raw/", "research/api/", "research/_", "_business/")
# research 下的数据与产物：本机运行时才生成，仓库只放脚本
RUNTIME_EXT = (".json", ".jsonl", ".csv", ".xlsx", ".txt", ".log", ".html", ".db")


def e10_docs_consistency(repo: str) -> Result:
    r = Result("E10", "文档与代码不矛盾（引用存在的文件/命令）")
    src_root = os.environ.get("PREPUB_SRC_ROOT", ROOT)
    for rel, p in walk(repo):
        if not rel.lower().endswith(".md"):
            continue
        txt = read_text(p)
        for m in re.finditer(r"`(research/[A-Za-z0-9_./\-]+\.(?:py|mjs|js|ps1))`", txt):
            target = m.group(1)
            alts = {target}
            head, _, tail = target.partition("/")
            if "client_" in tail:
                alts.add(head + "/" + tail.replace("client_", "client_", 1))
            if "client_" in tail:
                alts.add(head + "/" + tail.replace("client_", "client_", 1))
            if target.startswith(NONEXPORT_PREFIX) or (
            target.startswith("research/") and target.lower().endswith(RUNTIME_EXT)
        ):
                continue
            if not any(os.path.exists(os.path.join(repo, a.replace("/", os.sep))) or
                       os.path.exists(os.path.join(src_root, a.replace("/", os.sep))) for a in alts):
                r.fail(f"{rel} 引用了不存在的脚本：{target}")
        for bad in ("cargo build --release --offline", "npm install -g"):
            if bad in txt:
                r.fail(f"{rel} 含不可用命令：{bad}")
    return r


EMOJI_RE = re.compile("[\u2600-\u27bf\U0001f300-\U0001f9af\u2705\u274c\u2714\u2716]")
BOLD_LINE = re.compile(r"^\s*\*\*[^*]+\*\*\s*$", re.M)


def e11_doc_style(repo: str) -> Result:
    """文档风格：不写 emoji、不用破折号替标点、不整行加粗。这些是模板腔最明显的痕迹。"""
    r = Result("E11", "文档风格统一（无 emoji / 无破折号 / 无整行加粗）")
    bad = 0
    for rel, full in walk(repo):
        if not rel.lower().endswith(".md"):
            continue
        for i, line in enumerate(read_text(full).splitlines(), 1):
            if line.lstrip().startswith("```"):
                break
            if EMOJI_RE.search(line):
                r.fail(f"{rel}:{i} 出现 emoji")
                bad += 1
            if "——" in line:
                r.fail(f"{rel}:{i} 用破折号代标点")
                bad += 1
            if BOLD_LINE.match(line):
                r.fail(f"{rel}:{i} 整行加粗当小标题")
                bad += 1
            if bad > 40:
                r.note = "命中过多，已截断"
                return r
    r.note = ": 无命中" if bad == 0 else ""
    return r


def e13_imports(repo: str) -> Result:
    """导出仓里每个 .py 依赖的本地模块都必须真的在树里（try/except 包裹的可选依赖除外）。

    漏判过一次：wxwork_db_decrypt.py 按「取证工具不出公开仓」被 drop 掉，
    但 wxwork_unread_build.py / wxwork_todo_local.py 仍在 import 它 ——
    E10 只查文档引用的脚本，查不到 import，装完直接 ImportError。
    """
    r = Result("E13", "导出仓 Python 依赖完整（本地 import 都能解析）")
    files = [rel for rel, _ in walk(repo)]
    local = set()
    for rel in files:
        if rel.endswith('.py'):
            local.add(os.path.splitext(os.path.basename(rel))[0])
    std = set(getattr(sys, 'stdlib_module_names', ()))
    third = {'yaml', 'PIL', 'openpyxl', 'requests', 'numpy', 'pandas', 'docx', 'pptx',
             'win32api', 'win32con', 'win32gui', 'pythoncom', 'win32com'}
    n_py = 0
    for rel in files:
        if not rel.endswith('.py'):
            continue
        n_py += 1
        src = read_text(os.path.join(repo, rel))
        try:
            tree = ast.parse(src)
        except SyntaxError as exc:
            r.fail(f'{rel}: 语法错误 {exc}')
            continue
        optional = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.Try):
                for sub in ast.walk(node):
                    if isinstance(sub, (ast.Import, ast.ImportFrom)):
                        optional.add(id(sub))
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                names = [a.name.split('.')[0] for a in node.names]
            elif isinstance(node, ast.ImportFrom):
                names = [(node.module or '').split('.')[0]]
            else:
                continue
            for name in names:
                if not name or name in std or name in third or name in local:
                    continue
                if id(node) in optional:
                    continue          # try/except 包裹 = 可选依赖，允许缺
                r.fail(f'{rel}: import {name}（树里没有这个模块，且不在 try/except 里）')
    r.note = f'{len(local)} 个本地模块 / {n_py} 个 .py'
    return r

CHECKS = [
    ("E01", e01_guard), ("E02", e02_leak_words), ("E03", e03_abs_paths), ("E04", e04_placeholder),
    ("E05", e05_docs), ("E07", e07_install_chain), ("E08", e08_junk), ("E09", e09_size_binary),
    ("E10", e10_docs_consistency),
    ("E11", e11_doc_style), ("E12", e12_csharp), ("E13", e13_imports),
]


def run_all(repo: str, version: str | None) -> list[Result]:
    out: list[Result] = []
    for code, fn in CHECKS:
        try:
            out.append(fn(repo))
        except Exception as exc:                                # noqa: BLE001
            r = Result(code, getattr(fn, "__name__", code))
            r.fail(f"检查自身异常：{exc}")
            out.append(r)
    try:
        out.append(e06_version(repo, version))
    except Exception as exc:                                    # noqa: BLE001
        r = Result("E06", "版本一致性")
        r.fail(f"检查自身异常：{exc}")
        out.append(r)
    return sorted(out, key=lambda x: x.code)


def report(results: list[Result], repo: str) -> int:
    width = max(len(r.title) for r in results)
    print(f"\n发布前检查 · 目标树 {repo}")
    print("=" * (width + 26))
    for r in results:
        mark = "PASS" if r.ok else f"FAIL({len(r.bad)})"
        print(f"{r.code}  {r.title:<{width}}  {mark}")
        if r.note:
            print(f"      · {r.note}")
        for b in r.bad[:8]:
            print(f"      x {b}")
        if len(r.bad) > 8:
            print(f"      … 另有 {len(r.bad) - 8} 条")
    fails = [r for r in results if not r.ok]
    print("=" * (width + 26))
    print(f"PREPUBLISH_RESULT={'PASS' if not fails else 'FAIL'}  checks={len(results)} failed={len(fails)}")
    return 0 if not fails else 1


# ---------------------------------------------------------------- 自检（变异测试）

MUTATIONS = [    ("E12", "把 C# 采集器改成不可编译", lambda d: _break_csharp(d)),

    ("E02", "把真实业务词塞进 README",
     lambda d: _append(os.path.join(d, "README.md"), "\n<!-- LEAKCANARY911 -->\n")),
    ("E03", "塞一条本机绝对路径",
     lambda d: _append(os.path.join(d, "README.md"), "\n路径示例：" + "C" + ":" + chr(92) + "Users" + chr(92) + "audit" + chr(92) + "board.json\n")),
    ("E04", "把 manifest 的 bytes 改成字符串", lambda d: _mutate_manifest(d, lambda j: _corrupt_bytes(j))),
    ("E05", "制造重复章节标题", lambda d: _dup_heading(d)),
    ("E08", "塞一个调试日志", lambda d: _write(os.path.join(d, "_gout.txt"), "FAIL [占位符] ...\n")),
    ("E09", "塞一个未声明二进制", lambda d: _write(os.path.join(d, "x.exe"), b"MZ\x90\x00" * 8)),
    ("E10", "文档引用不存在的脚本", lambda d: _append(os.path.join(d, "README.md"),
                                                "\n见 `research/ghost_script.py`。\n")),
    ("E06", "改 CHANGELOG 顶部版本使与发布参数冲突", lambda d: _bump_changelog(d)),
    ("E01", "让 kit 副本与真源不一致（PARITY 应报红）", lambda d: _break_parity(d)),
    ("E07", "把安装顺序写反", lambda d: _reverse_order(d)),
]


def _peek_leak(d: str) -> str:
    try:
        words = [w.strip() for w in read_text(LEAK_FILE).splitlines() if w.strip() and not w.startswith("#")]
        return words[0] if words else "LEAK"
    except OSError:
        return "LEAK"



def _dup_heading(d: str) -> None:
    """复制 README 里第一个标题并原样再追加一次（参数是仓库目录）。"""
    path = d if d.lower().endswith(".md") else os.path.join(d, "README.md")
    raw = io.open(path, encoding="utf-8", newline="").read().replace(chr(13), "")
    lines = raw.split(chr(10))
    head = next((l.strip() for l in lines if l.startswith("## ")), "## 重复章节")
    with io.open(path, "a", encoding="utf-8", newline="") as f:
        f.write(chr(10) + head + chr(10) * 2 + "第一段" + chr(10) * 2 + head + chr(10) * 2 + "第二段" + chr(10))

def _append(p: str, s: str) -> None:
    with open(p, "a", encoding="utf-8", newline="") as fh:
        fh.write(s)


def _write(p: str, data) -> None:
    mode = "wb" if isinstance(data, bytes) else "w"
    with open(p, mode) as fh:
        fh.write(data)


def _break_parity(d: str) -> None:
    p = os.path.join(d, "skills", "project-board-kit", "files", "rust-widget", "src", "main.rs")
    if os.path.exists(p):
        _append(p, "\n// drift\n")


def _reverse_order(d: str) -> None:
    p = os.path.join(d, "skills", "project-board-kit", "SKILL.md")
    txt = read_text(p)
    txt2 = txt.replace("`setup.ps1` → `discover.ps1` → `doctor.ps1`",
                       "`discover.ps1` → `doctor.ps1` → `setup.ps1`")
    if txt2 == txt:
        txt2 = txt.replace("setup.ps1 → discover.ps1 → doctor.ps1",
                           "discover.ps1 → doctor.ps1 → setup.ps1")
    with open(p, "w", encoding="utf-8", newline="") as fh:
        fh.write(txt2)


def _bump_changelog(d: str) -> None:
    p = os.path.join(d, "CHANGELOG.md")
    txt = read_text(p)
    m = VERSION_HEAD.search(txt)
    if not m:
        _append(p, "\n## v9.9 — 2030-01-01\n\n注入\n")
        return
    txt = txt[:m.start()] + "## v9.9 — 2030-01-01" + txt[m.end():]
    with open(p, "w", encoding="utf-8", newline="") as fh:
        fh.write(txt)


def _break_csharp(d: str) -> None:
    for dirpath, _dirs, names in os.walk(d):
        for n in names:
            if n.endswith(".cs") and os.sep + "research" + os.sep in dirpath:
                p = os.path.join(dirpath, n)
                txt = open(p, "r", encoding="utf-8", errors="replace").read()
                open(p, "w", encoding="utf-8").write("class __Broken { void M() { var x = new byte[CLIENT_B]; } }\n" + txt)
                return


def _mutate_manifest(d: str, fn) -> None:
    p = os.path.join(d, "skills", "project-board-kit", "files", "manifest.json")
    with open(p, encoding="utf-8") as fh:
        j = json.load(fh)
    j = fn(j) or j
    with open(p, "w", encoding="utf-8") as fh:
        json.dump(j, fh, ensure_ascii=False, indent=2)


def _corrupt_bytes(j):
    for k in list((j.get("files") or {}).keys())[:1]:
        j["files"][k]["bytes"] = "ITEM_X"
    return j


def _force_rmtree(path: str) -> None:
    import shutil
    import stat

    def _onerr(func, p_, _exc):
        try:
            os.chmod(p_, stat.S_IWRITE)
            func(p_)
        except Exception:
            pass

    shutil.rmtree(path, onerror=_onerr)


def selftest(repo: str, version: str | None) -> int:
    base = run_all(repo, version)
    base_bad = {r.code for r in base if not r.ok}
    if base_bad:
        print(f"自检前置：干净树本身不绿 {sorted(base_bad)}，先修干净树再跑 --selftest")
        return 2
    tmp = tempfile.mkdtemp(prefix="prepub_selftest_" + uuid.uuid4().hex[:8] + "_")
    bad = 0
    try:
        for code, desc, mutate in MUTATIONS:
            d = os.path.join(tmp, code)
            shutil.copytree(repo, d, ignore=shutil.ignore_patterns(".git"))
            if code == "E02":                      # 给 E02 准备一份仓库外词表（canary），否则该检查在公开仓恒跳过
                wl = os.path.join(tmp, "canary_words.txt")
                _write(wl, "LEAKCANARY911\n")
                os.environ["PREPUB_LEAK_FILE"] = wl
            mutate(d)
            res = {r.code: r for r in run_all(d, version)}
            r = res.get(code)
            if r is None:
                print(f"SELFTEST {code} 未定义检查项 —— 盲区！")
                bad += 1
            elif r.ok:
                print(f"SELFTEST {code} FAIL：注入「{desc}」后仍报 PASS —— 该检查无效！")
                bad += 1
            else:
                print(f"SELFTEST {code} ok  （{desc} → {r.bad[0][:80]}）")
            os.environ.pop("PREPUB_LEAK_FILE", None)
    finally:
        _force_rmtree(tmp)
    print(f"SELFTEST_RESULT={'PASS' if not bad else 'FAIL'}  mutations={len(MUTATIONS)} blind={bad}")
    return 0 if not bad else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=DEFAULT_REPO)
    ap.add_argument("--version", default=None)
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    repo = os.path.abspath(a.repo)
    # 在业务工作树里裸跑会把产品图/文档风格全算成违规（260+ 条假红），一次误报顶十次
    # 真排查：这类目标应走 _business/selfcheck.py（它只在导出仓里跑本检查）。
    if not os.path.isfile(os.path.join(repo, 'scripts', 'policy.py')):
        print('PREPUBLISH_SKIPPED 目标不是导出仓（缺 scripts/policy.py）：%s' % repo)
        print('  业务工作树请用：python -X utf8 _business/selfcheck.py')
        return 0
    if not os.path.isdir(repo):
        print(f"目标树不存在：{repo}")
        return 2
    if a.selftest:
        ver = a.version
        if not ver:
            m = VERSION_HEAD.search(read_text(os.path.join(repo, "CHANGELOG.md")))
            ver = "v" + m.group(1) if m else None
        return selftest(repo, ver)
    return report(run_all(repo, a.version), repo)


if __name__ == "__main__":
    sys.exit(main())
