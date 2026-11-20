#!/usr/bin/env python
# -*- coding: utf-8 -*-
u"""发布链唯一策略源（二进制判定 / 白名单 / 忽略规则 / 路径判定）。

为什么集中：这些规则原本散在四处（导出 skip 列表、guard BIN_EXT、prepublish BIN_ALLOW、
doctor 归一化），每加一个二进制要改四个文件，v2.26.0 连挂六次全部源于此。
本文件随仓库分发（`scripts/policy.py`），本机工具通过 `_business/policy.py` 引用同一份。

约定：路径一律用仓库相对、正斜杠。
"""
import hashlib
import os

# ---- 二进制扩展名：不做文本层扫描（敏感词/盘符/hash 归一化都跳过） ----
BIN_EXT = ('.exe', '.dll', '.whl', '.zip', '.png', '.jpg', '.jpeg', '.ico', '.pdf', '.db', '.so', '.dylib')

# ---- 允许随包的二进制（其余二进制不入库、不入包） ----
# 必须随包的：企微只读密钥扫描器的运行库（无法现场编译）
BIN_ALLOW = {
    'skills/project-board-kit/files/vendor/libcrypto32.dll',
    'skills/project-board-kit/files/research/em_cookie_scan.exe',
}

# ---- 导出/入库都会忽略的垃圾模式 ----
JUNK_DIRS = ('_dd', '_builddist', '_export', '_kit_smoke', '_review', '_ghcheck', '_vfy',
             '__pycache__', 'target', '.finesse')
JUNK_FILES = ('.log', '.pyc')

# ---- 文本层扫描的扩展名（其余一律跳过） ----
TEXT_EXT = ('.py', '.mjs', '.js', '.ps1', '.cs', '.md', '.toml', '.rs', '.json', '.yml', '.yaml')


def is_bin(rel):
    return rel.lower().endswith(BIN_EXT)


def bin_allowed(rel):
    return rel.replace('\\', '/') in BIN_ALLOW


def is_junk(rel):
    rel = rel.replace('\\', '/')
    parts = rel.split('/')
    if any(p in JUNK_DIRS for p in parts[:-1]) or parts[0] in JUNK_DIRS:
        return True
    return rel.lower().endswith(JUNK_FILES)


def norm_bytes(data, rel):
    u"""hash 口径：文本去 BOM、CRLF->LF；二进制原样。"""
    if is_bin(rel):
        return data
    if data.startswith(b'\xef\xbb\xbf'):
        data = data[3:]
    return data.replace(b'\r\n', b'\n')


def sha_file(path, rel=None):
    rel = rel if rel is not None else os.path.basename(path)
    with open(path, 'rb') as f:
        return hashlib.sha256(norm_bytes(f.read(), rel)).hexdigest()
