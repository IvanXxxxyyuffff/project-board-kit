#!/usr/bin/env python
# -*- coding: utf-8 -*-
u"""一条命令跑完本仓库全部门禁，一次列全失败项。

为什么存在：发布链的失败信息分散在 preflight / guard / prepublish 三步，
逐条发现会浪费多轮往返（历史教训：同一版本连挂多次只为看下一条错误）。

用法：
  python scripts/selfcheck.py          # 有失败退出码 1
  python scripts/selfcheck.py --fix    # 允许自愈项（副本同步/清单重建/垃圾脱链）

退出码：0 全绿；1 有失败。
"""
import os
import subprocess
import sys

# 本脚本会 import 导出仓的 scripts/*.py；不关字节码写入就会把 __pycache__ 写进导出树，
# 而 E08 判红 → 出现「selfcheck 全绿、prepublish 必红」的假绿（S4 排在 S0 之前）。
sys.dont_write_bytecode = True

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
PY = sys.executable
ENV = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
STEPS = []


def run(name, args, cwd=None, expect=None):
    r = subprocess.run([PY, "-X", "utf8"] + args, cwd=cwd or ROOT, env=ENV,
                       capture_output=True, text=True, errors="replace")
    out = (r.stdout or "") + (r.stderr or "")
    ok = r.returncode == 0 and all(e in out for e in (expect or []))
    STEPS.append((name, ok, out))
    return ok


def policy_selftest():
    u"""���径回归：PE/wheel 必须原样，文本必须归一。"""
    sys.path.insert(0, HERE)
    detail = []
    ok = True
    try:
        import policy
        mz = b"MZ" + b"\x00" * 60 + b"PE\x00\x00" + b"\r\n" * 4
        pk = b"PK\x03\x04" + b"\r\n" * 8
        if policy.norm_bytes(mz, "a.exe") != mz:
            ok = False
            detail.append("policy.norm_bytes 未对 PE 保持原始字节")
        if policy.norm_bytes(pk, "a.whl") != pk:
            ok = False
            detail.append("policy.norm_bytes 未对 wheel 保持原始字节")
        if policy.norm_bytes(b"a\r\nb", "a.txt") != b"a\nb":
            ok = False
            detail.append("policy.norm_bytes 未对文本做 CRLF 归一")
    except Exception as exc:  # noqa: BLE001
        ok = False
        detail.append("导入 policy 失败：%s" % exc)
    try:
        sys.path.insert(0, HERE)
        import guard_meta
        if guard_meta.norm_bytes(b"MZ\r\n\r\n") != b"MZ\r\n\r\n":
            ok = False
            detail.append("guard_meta.norm_bytes 未对 PE 保持原始字节")
    except Exception as exc:  # noqa: BLE001
        ok = False
        detail.append("导入 guard_meta 失败：%s" % exc)
    STEPS.append(("S0 二进制口径回归", ok, "\n".join(detail)))
    return ok


def _exit_code(bad):
    u"""有失败就返回 1：门禁必须能阻断，不允许用「恒 0」掩盖问题。"""
    return 0 if not bad else 1


def main():
    fix = "--fix" in sys.argv
    run("S1 重建清单", ["scripts/gen_manifest.py"], expect=["MANIFEST files="])
    # 源树卫生（副本漂移/垃圾入库/清单过期）属发布者本地治理，
    # 依赖业务源树与 git 索引，不在仓库内复跑，避免在干净 clone 上误报。
    if fix:
        print("    提示：--fix 仅重建清单；源树卫生请在你自己的发布工作树里做")
    run("S3 门禁 guard", ["scripts/guard_meta.py"], expect=["GUARD_RESULT=PASS"])
    run("S4 发布前检查 prepublish", ["scripts/prepublish_gate.py"],
        expect=["PREPUBLISH_RESULT=PASS"])
    policy_selftest()

    bad = [s for s in STEPS if not s[1]]
    print("")
    print("=" * 66)
    for name, ok, out in STEPS:
        print("%-30s %s" % (name, "PASS" if ok else "FAIL"))
        if not ok:
            for line in [l for l in out.splitlines() if l.strip()][-12:]:
                print("    " + line)
    print("=" * 66)
    print("SELFCHECK_RESULT=%s steps=%d failed=%d"
          % ("PASS" if not bad else "FAIL", len(STEPS), len(bad)))
    return _exit_code(bad)


if __name__ == "__main__":
    sys.exit(main())
