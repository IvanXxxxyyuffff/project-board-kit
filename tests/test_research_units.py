"""research/ 纯函数单元测试（stdlib unittest，无第三方依赖）。

跑法：
    python -m unittest discover -s tests -v
    python -m unittest tests.test_research_units -v

这些测试只覆盖「不需要外部数据文件、不需要网络」的纯逻辑：
解析结果归一、时间窗切片、Excel 序列号/月日转日期。
真实抓取与渲染由三层治具覆盖（见 docs/20-检测治具.md）。
"""
import datetime as dt
import os
import sys
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "research"))

import ec_meeting_fetch as emf          # noqa: E402
import sheet_canon as sc                # noqa: E402


class TestNormEvents(unittest.TestCase):
    def test_maps_fields_and_truncates(self):
        res = {"events": [{
            "id": 42,
            "begindate": "2026-03-04 00:00:00",
            "begintime": "09:30:00",
            "endtime": "10:15:00",
            "name": "  周会  ",
            "status": "1",
        }]}
        rows = emf.norm_events(res)
        self.assertEqual(len(rows), 1)
        r = rows[0]
        self.assertEqual(r["id"], "42")
        self.assertEqual(r["date"], "2026-03-04")
        self.assertEqual(r["start"], "09:30")
        self.assertEqual(r["end"], "10:15")
        self.assertEqual(r["name"], "周会")

    def test_drops_rows_without_date_or_name(self):
        res = {"events": [
            {"begindate": "", "name": "x"},
            {"begindate": "2026-03-04", "name": ""},
            {"begindate": "2026-03-04", "name": "ok"},
        ]}
        self.assertEqual([r["name"] for r in emf.norm_events(res)], ["ok"])

    def test_empty(self):
        self.assertEqual(emf.norm_events({}), [])
        self.assertEqual(emf.norm_events({"events": None}), [])


class TestSliceFor(unittest.TestCase):
    def test_window_and_sort(self):
        items = [
            {"date": "2026-03-05", "start": "10:00", "name": "b"},
            {"date": "2026-03-01", "start": "08:00", "name": "before"},
            {"date": "2026-03-03", "start": "09:00", "name": "a"},
            {"date": "2026-03-03", "start": "", "name": "no-time"},
            {"date": "2026-03-20", "start": "08:00", "name": "after"},
        ]
        got = emf.slice_for(items, dt.date(2026, 3, 3), dt.date(2026, 3, 5))
        self.assertEqual([r["name"] for r in got], ["a", "no-time", "b"])


class TestSerialToMd(unittest.TestCase):
    def test_excel_serial_with_hint(self):
        got = sc.serial_to_md("46303(10/8)")
        self.assertRegex(got, r"^\d{4}-\d{2}-\d{2}$")
        self.assertEqual(got, (dt.date(1899, 12, 30) + dt.timedelta(days=46303)).isoformat())

    def test_month_day_picks_not_future_year(self):
        today = dt.date(2026, 11, 17)
        self.assertEqual(sc.serial_to_md("3/4", today=today), "2026-03-04")
        self.assertEqual(sc.serial_to_md("12/25", today=today), "2025-12-25")

    def test_invalid(self):
        self.assertEqual(sc.serial_to_md(""), "")
        self.assertEqual(sc.serial_to_md("not-a-date"), "")


if __name__ == "__main__":
    unittest.main(verbosity=2)
