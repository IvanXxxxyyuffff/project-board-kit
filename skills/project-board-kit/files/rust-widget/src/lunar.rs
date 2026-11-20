//! 农历（BCL ChineseLunisolarCalendar 生成的月表，2020–2035）+ 二十四节气（21 世纪通用公式）。
//! 不做调休表（原版内嵌 2024–2026 共 110 行，无法算法推导，一期不还原）。

#[allow(dead_code)]
pub struct LunarYear {
    pub y: i32,
    /// 闰月序号（1..=13，0=无闰月）；序号 L 表示「闰(L-1)月」
    pub leap: u8,
    pub months: &'static [u8],
    /// 正月初一（公历）
    pub new_year: (i32, u32, u32),
}

pub const LUNAR: &[LunarYear] = &[
    LunarYear { y: 2020, leap: 5, months: &[29,30,30,30,29,30,29,29,30,29,30,29,30], new_year: (2020,1,25) },
    LunarYear { y: 2021, leap: 0, months: &[29,30,30,29,30,29,30,29,30,29,30,29], new_year: (2021,2,12) },
    LunarYear { y: 2022, leap: 0, months: &[30,29,30,29,30,30,29,30,29,30,29,30], new_year: (2022,2,1) },
    LunarYear { y: 2023, leap: 3, months: &[29,30,29,29,30,30,29,30,30,29,30,29,30], new_year: (2023,1,22) },
    LunarYear { y: 2024, leap: 0, months: &[29,30,29,29,30,29,30,30,29,30,30,29], new_year: (2024,2,10) },
    LunarYear { y: 2025, leap: 7, months: &[30,29,30,29,29,30,29,30,29,30,30,30,29], new_year: (2025,1,29) },
    LunarYear { y: 2026, leap: 0, months: &[30,29,30,29,29,30,29,29,30,30,30,29], new_year: (2026,2,17) },
    LunarYear { y: 2027, leap: 0, months: &[30,30,29,30,29,29,30,29,29,30,30,29], new_year: (2027,2,6) },
    LunarYear { y: 2028, leap: 6, months: &[30,30,30,29,30,29,29,30,29,29,30,30,29], new_year: (2028,1,26) },
    LunarYear { y: 2029, leap: 0, months: &[30,30,29,30,29,30,29,30,29,29,30,30], new_year: (2029,2,13) },
    LunarYear { y: 2030, leap: 0, months: &[29,30,29,30,30,29,30,29,30,29,30,29], new_year: (2030,2,3) },
    LunarYear { y: 2031, leap: 4, months: &[29,30,30,29,30,29,30,30,29,30,29,30,29], new_year: (2031,1,23) },
    LunarYear { y: 2032, leap: 0, months: &[30,29,29,30,29,30,30,29,30,30,29,30], new_year: (2032,2,11) },
    LunarYear { y: 2033, leap: 12, months: &[29,30,29,29,30,29,30,29,30,30,30,29,30], new_year: (2033,1,31) },
    LunarYear { y: 2034, leap: 0, months: &[29,30,29,29,30,29,30,29,30,30,29,30], new_year: (2034,2,19) },
    LunarYear { y: 2035, leap: 0, months: &[30,29,30,29,29,30,29,29,30,30,29,30], new_year: (2035,2,8) },
];

/// 公历 → 儒略日式天数（1970-01-01 = 0），Howard Hinnant 算法。
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i32 - 719468
}

#[allow(dead_code)]
pub fn civil_from_days(z: i32) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0=周日 … 6=周六
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    ((days_from_civil(y, m, d) + 4).rem_euclid(7)) as u32
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

#[allow(dead_code)]
pub fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 农历：(月 1..12, 日 1..30, 是否闰月)
pub fn lunar_of(y: i32, m: u32, d: u32) -> Option<(u8, u8, bool)> {
    let day = days_from_civil(y, m, d);
    // 找覆盖该公历日的农历年
    let rec = LUNAR.iter().find(|r| {
        let ny = days_from_civil(r.new_year.0, r.new_year.1, r.new_year.2);
        let total: i32 = r.months.iter().map(|&x| x as i32).sum();
        day >= ny && day < ny + total
    })?;
    let ny = days_from_civil(rec.new_year.0, rec.new_year.1, rec.new_year.2);
    let mut off = day - ny;
    let ordinal;
    for (i, &len) in rec.months.iter().enumerate() {
        if off < len as i32 {
            ordinal = (i + 1) as u8;
            let leap = rec.leap != 0 && ordinal == rec.leap;
            let month = if rec.leap != 0 && ordinal > rec.leap {
                ordinal - 1
            } else {
                ordinal
            };
            let month = if leap { ordinal - 1 } else { month };
            return Some((month, (off + 1) as u8, leap));
        }
        off -= len as i32;
    }
    None
}

const DAY_NAMES: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一", "十二",
    "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十", "廿一", "廿二", "廿三", "廿四",
    "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];

const MONTH_NAMES: [&str; 12] = [
    "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月", "冬月", "腊月",
];

pub fn day_name(d: u8) -> &'static str {
    DAY_NAMES[(d.clamp(1, 30) - 1) as usize]
}

pub fn month_name(m: u8, leap: bool) -> String {
    let base = MONTH_NAMES[((m.clamp(1, 12)) - 1) as usize];
    if leap {
        format!("闰{}", base)
    } else {
        base.to_string()
    }
}

/// 月历格子里的农历短标签：初一显示月名，其余显示日名。
pub fn cell_label(y: i32, m: u32, d: u32) -> Option<String> {
    let (lm, ld, leap) = lunar_of(y, m, d)?;
    Some(if ld == 1 { month_name(lm, leap) } else { day_name(ld).to_string() })
}

/// 24 节气（21 世纪 C 值）。返回该日节气名。
pub fn jieqi(y: i32, m: u32, d: u32) -> Option<&'static str> {
    if !(2000..2100).contains(&y) {
        return None;
    }
    const TERMS: [[(&str, f64); 2]; 12] = [
        [("小寒", 5.4055), ("大寒", 20.12)],
        [("立春", 3.87), ("雨水", 18.73)],
        [("惊蛰", 5.63), ("春分", 20.646)],
        [("清明", 4.81), ("谷雨", 20.1)],
        [("立夏", 5.52), ("小满", 21.04)],
        [("芒种", 5.678), ("夏至", 21.37)],
        [("小暑", 7.108), ("大暑", 22.83)],
        [("立秋", 7.5), ("处暑", 23.13)],
        [("白露", 7.646), ("秋分", 23.042)],
        [("寒露", 8.318), ("霜降", 23.438)],
        [("立冬", 7.438), ("小雪", 22.36)],
        [("大雪", 7.18), ("冬至", 21.94)],
    ];
    let yy = (y % 100) as f64;
    let leap = ((y - 1) % 100) / 4;
    let pair = TERMS[((m.clamp(1, 12)) - 1) as usize];
    for (name, c) in pair {
        let day = (yy * 0.2422 + c).floor() as i32 - leap;
        if day == d as i32 {
            return Some(name);
        }
    }
    None
}

/// 综合标签：节气优先（原版 DisplayHoliday 语义），否则农历。
pub fn label(y: i32, m: u32, d: u32) -> Option<String> {
    if let Some(t) = jieqi(y, m, d) {
        return Some(t.to_string());
    }
    cell_label(y, m, d)
}
