#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""secret_scan.py —— 通用秘密扫描（公开仓 CI 与本地都能跑，不需要私有词表）

与 prepublish_gate.py 的分工：

  * prepublish_gate.py 的 E02 用**私有词表**（`_business/leak_words.txt`，业务名 /
    客户名 / SKU / 内网主机），只在发布机上跑，词表绝不进仓库；
  * 本脚本用**通用模式**（令牌形态、私钥块、cookie 名、邮箱、手机号、内网 IP、
    用户目录、盘符），模式本身公开，任何 clone 都能自查，所以可以直接挂在 CI。

GitHub 免费公开仓只提供「厂商格式扫描 + push protection」；企业内网凭据、
企微 cookie 名、自建服务 token 这些非厂商形态要靠这一层补。付费的
Secret Protection（non-provider patterns / validity checks）本账号不可用。

用法：
    python scripts/secret_scan.py                 # 扫仓库根（默认）
    python scripts/secret_scan.py --root .        # 指定根
    python scripts/secret_scan.py --quiet

退出码：0 = 无命中；1 = 有命中。
"""
from __future__ import annotations

import argparse
import os
import re
import sys

sys.dont_write_bytecode = True

SKIP_DIRS = {
    ".git", "__pycache__", "node_modules", "target", ".venv", "venv",
    "_builddist", "_export", ".agent-teams", "dist",
}
TEXT_EXT = {
    ".py", ".mjs", ".js", ".cjs", ".ts", ".rs", ".ps1", ".psm1", ".bat", ".cmd",
    ".md", ".txt", ".json", ".jsonl", ".yml", ".yaml", ".toml", ".csv", ".tsv",
    ".html", ".css", ".cs", ".xml", ".ini", ".cfg", ".gitignore", ".gitattributes",
    ".sh", ".sql", ".example",
}
MAX_BYTES = 4 * 1024 * 1024

# 允许出现的字面量：回环地址、文档占位符、版本号形如 0.1.0.0
ALLOW_LINE = (
    re.compile(r"127\.0\.0\.1"),
    re.compile(r"0\.0\.0\.0"),
    re.compile(r"localhost", re.I),
    re.compile(r"<[^<>]{1,40}>"),            # <必填：…> / <IP> / <TOKEN>
    re.compile(r"\.\.\.{1,3}"),              # 省略号占位
    re.compile(r"\b\d+\.\d+\.\d+\.\d+\b.*(?:version|Version|APP_VERSION|manifest)"),
)

RULES: list[tuple[str, re.Pattern[str]]] = [
    # —— 厂商令牌 ——
    ("github-token", re.compile(r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}")),
    ("github-pat", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}")),
    ("aws-key", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("slack-token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}")),
    ("google-key", re.compile(r"\bAIza[0-9A-Za-z_\-]{35}\b")),
    ("private-key", re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |PGP )?PRIVATE KEY-----")),
    ("jwt", re.compile(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{6,}")),
    # —— 企微 / 文档 cookie 名（本项目最敏感的一类）——
    ("wecom-cookie", re.compile(r"\b(?:wedoc_sid|wedoc_sids|wedoc_skey|wedoc_ticket|tdoc_uid|wwrtx\.vid|wksid)\b\s*[:=]", re.I)),
    ("cookie-assign", re.compile(r"(?:^|[^A-Za-z])(?:cookies?|Cookie)\s*[:=]\s*['\"][^'\"]{24,}['\"]")),
    ("auth-header", re.compile(r"Authorization\s*:\s*(?:Basic|Bearer)\s+[A-Za-z0-9+/=._-]{16,}")),
    # —— 内网与主机 ——
    ("private-ipv4", re.compile(r"\b(?:10\.\d{1,3}|192\.168|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b")),
    ("weaver-host", re.compile(r"https?://[A-Za-z0-9._-]+\.weaver\.com\.cn")),
    # —— 本机路径 ——
    ("win-user-path", re.compile(r"[A-Za-z]:\\\\?Users\\\\?[A-Za-z0-9._-]+")),
    ("drive-path", re.compile(r"(?<![\w/])[A-Za-z]:\\\\?(?:PRODUCT_LINE项目|Tools|moli|Temp|rt-widget)")),
    # —— 个人数据 ——
    ("email", re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")),
    # 手机号：两侧不能贴字母/数字，否则 sha256 十六进制里随便一段 11 位数字就会误报
    ("cn-mobile", re.compile(r"(?<![0-9A-Za-z])1[3-9]\d{9}(?![0-9A-Za-z])")),
]

# 整行是哈希 / base64 的值时不做模式匹配（manifest 里的 sha256 常含 11 位数字串）
HASH_LINE = re.compile(r"^[\s\"'{}:,]*[0-9a-fA-F]{32,}[\s\"'{}:,]*$")
HASH_FIELD = re.compile(r"[\"'](?:sha256|sha1|md5|hash|blob|etag)[\"']\s*:\s*[\"'][0-9a-fA-F]{16,}[\"']", re.I)

# 允许的例子：文档里讲「长什么样」的示例值
ALLOW_VALUE = (
    re.compile(r"example\.com", re.I),
    re.compile(r"user@example"),
    re.compile(r"xxx+", re.I),
    re.compile(r"your[_-]?token", re.I),
)


def iter_text(root: str):
    for base, dirs, names in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for n in names:
            p = os.path.join(base, n)
            ext = os.path.splitext(n)[1].lower()
            if ext not in TEXT_EXT and n not in (".gitignore", ".gitattributes"):
                continue
            try:
                if os.path.getsize(p) > MAX_BYTES:
                    continue
            except OSError:
                continue
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            yield rel, p


def allowed(line: str) -> bool:
    if any(p.search(line) for p in ALLOW_VALUE):
        return True
    # 只有回环 / 占位符的行直接放过
    stripped = line.strip()
    for p in ALLOW_LINE:
        stripped = p.sub("", stripped)
    return len(stripped) < 8


def scan(root: str) -> list[str]:
    hits: list[str] = []
    for rel, path in iter_text(root):
        try:
            with open(path, "r", encoding="utf-8", errors="replace") as fh:
                for i, line in enumerate(fh, 1):
                    if len(line) > 4000:
                        line = line[:4000]
                    if HASH_LINE.match(line) or HASH_FIELD.search(line):
                        continue
                    if allowed(line):
                        continue
                    for name, pat in RULES:
                        m = pat.search(line)
                        if m:
                            hits.append(f"{rel}:{i} [{name}] {m.group(0)[:60]}")
                            break
        except OSError as e:  # noqa: BLE001
            hits.append(f"{rel}:0 [read-error] {e}")
    return hits


def main() -> int:
    ap = argparse.ArgumentParser(description="通用秘密扫描（公开仓 CI 可用）")
    ap.add_argument("--root", default=".", help="扫描根目录，默认当前目录")
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--limit", type=int, default=40, help="最多打印多少条命中")
    a = ap.parse_args()

    root = os.path.abspath(a.root)
    files = sum(1 for _ in iter_text(root))
    hits = scan(root)
    if not hits:
        print(f"SECRET_SCAN_RESULT=PASS files={files}")
        return 0
    if not a.quiet:
        for h in hits[: a.limit]:
            print(f"SECRET_SCAN FAIL {h}")
        if len(hits) > a.limit:
            print(f"SECRET_SCAN … 另有 {len(hits) - a.limit} 条未打印")
    print(f"SECRET_SCAN_RESULT=FAIL files={files} hits={len(hits)}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
