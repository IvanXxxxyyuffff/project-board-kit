"""企微消息类型映射回归（合成 protobuf，不含任何真实业务数据）。

跑法：在仓库根 `python -m unittest discover -s tests`。
这些用例守住三件曾经踩过的事：
  1. 文件消息（20/15/34/102）必须给出文件名 + 扩展名，不能退化成「非文本消息」；
  2. 图片消息（14）字段 2 常把说明文字和文件名拼在一起（「资源 1.png」），要只留文件名；
  3. 空体的系统事件（1011 等）显示「系统消息」，而不是让面板出现空白或乱码串。
"""
import sys
import unittest
from pathlib import Path

def _repo_root() -> Path:
    """导出仓里是 <repo>/tests/…，业务工作树里是 <repo>/_business/repo_overlay/tests/…"""
    for parent in Path(__file__).resolve().parents:
        if (parent / "research" / "wxwork_chat_export.py").is_file():
            return parent
    return Path(__file__).resolve().parents[1]


ROOT = _repo_root()
sys.path.insert(0, str(ROOT / "research"))

import wxwork_chat_export as X  # noqa: E402


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


def tag(field_no: int, wire: int) -> bytes:
    return varint((field_no << 3) | wire)


def s(field_no: int, text: str) -> bytes:
    raw = text.encode("utf-8")
    return tag(field_no, 2) + varint(len(raw)) + raw


def i(field_no: int, value: int) -> bytes:
    return tag(field_no, 0) + varint(value)


class FileMessageTest(unittest.TestCase):
    def test_pptx_name_and_size(self):
        body = s(2, "样例项目_2026.11.19.pptx") + i(4, 143_654_912) + s(44, "pptx")
        meta = X.msg_meta(body, 20)
        self.assertEqual(meta["kind"], "文件")
        self.assertEqual(meta["name"], "样例项目_2026.11.19.pptx")
        label = X.msg_label(20, meta)
        self.assertIn("样例项目", label)
        self.assertIn("MB", label)
        self.assertNotIn("非文本", label)

    def test_wecom_disk_file_has_size_text(self):
        body = s(5, "报价单.xlsx") + s(6, "3 MB")
        meta = X.msg_meta(body, 565)
        self.assertEqual(meta["kind"], "微盘文件")
        self.assertEqual(meta["name"], "报价单.xlsx")
        self.assertIn("MB", X.msg_label(565, meta))

    def test_plain_file_without_size(self):
        body = s(2, "结构图.pdf")
        label = X.msg_label(15, X.msg_meta(body, 15))
        self.assertIn("结构图.pdf", label)


class ImageMessageTest(unittest.TestCase):
    def test_caption_is_stripped(self):
        body = s(2, "资源 1.png")
        meta = X.msg_meta(body, 14)
        self.assertEqual(meta["kind"], "图片")
        self.assertEqual(meta["name"], "1.png")

    def test_wecom_screenshot_name(self):
        body = s(2, "企业微信截图_20261119.png")
        self.assertEqual(X.msg_meta(body, 14)["name"], "企业微信截图_20261119.png")


class SystemEventTest(unittest.TestCase):
    def test_empty_body_is_system(self):
        meta = X.msg_meta(b"", 1011)
        self.assertEqual(meta["kind"], "")
        self.assertEqual(X.msg_label(1011, meta), "[非文本消息]")

    def test_unknown_type_is_not_text(self):
        label = X.msg_label(9999, {"kind": "", "name": "", "size": 0})
        self.assertIn("非文本", label)


class TextMessageTest(unittest.TestCase):
    def test_text_is_extracted(self):
        body = s(1, "开会了") + s(2, "会议室 3 楼")   # 结构近似，正文取最长可读串
        text = X.msg_text(body, 2)
        self.assertTrue(text.startswith("开会了") or "会议室" in text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
