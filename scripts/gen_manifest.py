#!/usr/bin/env python
# -*- coding: utf-8 -*-
u"""重建套件清单 manifest.json（实有清单：sha256 + 归一化字节数）。

规则集中在 scripts/policy.py：文本去 BOM、CRLF->LF；二进制按原始字节。
用法：python scripts/gen_manifest.py
"""
import datetime
import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import policy  # noqa: E402

ROOT = os.path.dirname(HERE)
KIT = os.path.join(ROOT, "skills", "project-board-kit")
FILES = os.path.join(KIT, "files")
OUT = os.path.join(FILES, "manifest.json")


def main():
    entries = {}
    for base, _dirs, names in os.walk(FILES):
        for n in sorted(names):
            p = os.path.join(base, n)
            rel = os.path.relpath(p, KIT).replace(os.sep, "/")
            if rel.endswith("manifest.json"):
                continue
            data = io_bytes(p)
            nb = policy.norm_bytes(data, rel)
            entries[rel] = {"sha256": hashlib.sha256(nb).hexdigest(), "bytes": len(nb)}
    doc = {"at": datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
           "files": dict(sorted(entries.items()))}
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(json.dumps(doc, ensure_ascii=False, indent=2))
    total = sum(v["bytes"] for v in entries.values())
    print("MANIFEST files=%d total_bytes=%d" % (len(entries), total))


def io_bytes(path):
    with open(path, "rb") as fh:
        return fh.read()


if __name__ == "__main__":
    main()
