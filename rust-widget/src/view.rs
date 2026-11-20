//! 视图状态：标签页、月份（翻月）、勾选、我的待办（部门导入 + 自建）、群聊信号、选中日期。
//! 纯逻辑层（不碰 Win32），预览 bin 与 t3 窗口层共用；搬进主 crate 时数据来源换成
//! `crate::data::WidgetData` + `crate::state::AppState`（department[] / custom[] / ui.tab）。

use std::collections::BTreeMap;

use crate::canvas::Rgba;
use crate::data::{Signal as DataSignal, WidgetData as Data};
use crate::hit::{Hit, Kind};
use crate::layout::{P0C, P1C, P2C};
use crate::lunar;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    /// 项目进度：抓取待办 + 项目节点日历 + 群聊信号（只读）
    Proj,
    /// 部门进度：我的待办（部门导入 + 自建）+ 待办 due 日历
    Dept,
    /// 会议安排：本人 Ecology 日程（当日 / 当周 / 当月）
    Meet,
}

impl Tab {
    pub fn key(self) -> &'static str {
        match self {
            Tab::Proj => "proj",
            Tab::Dept => "dept",
            Tab::Meet => "meet",
        }
    }
    pub fn from_key(s: &str) -> Tab {
        if s == "dept" {
            Tab::Dept
        } else if s == "meet" {
            Tab::Meet
        } else {
            Tab::Proj
        }
    }
}

/// 一行待办（部门导入 or 自建）。`dept=true` 时左侧画 3px 灰蓝竖条标记来源。
#[derive(Clone, Debug)]
pub struct DeptRow {
    pub text: String,
    pub due: String,
    pub done: bool,
    pub prio: String,
    pub img: String,
    pub note: String,
}

/// 会议提醒（开会前 5 分钟强制弹窗用）。
#[derive(Clone, Debug, Default)]
pub struct Remind {
    /// 会议开始时间 HH:MM
    pub time: String,
    pub name: String,
    /// 地点
    pub place: String,
    /// 参会人员
    pub people: String,
    /// 距开始还有几分钟
    pub left: i32,
}

/// 会议一行（会议安排标签）。
#[derive(Clone, Debug, Default)]
pub struct MeetRow {
    /// 地点（可能为空）
    pub place: String,
    /// 参会人员（逗号分隔，可能为空）
    pub people: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub name: String,
}

/// 归档一行（t10 `ArchiveItem` 的视图镜像）。
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ArcRow {
    pub id: String,
    pub text: String,
    pub due: String,
    pub done: bool,
    /// 归档时间，已格式化为 "M-D"
    pub at: String,
}

#[derive(Clone, Debug)]
pub struct SignalRow {
    pub t: String,
    pub sender: String,
    pub text: String,
    /// "EM" / "企微"，空串=旧数据不画角标
    pub src: String,
    /// "group" / "direct"
    pub kind: String,
    /// 群名（私聊为空）
    pub conv: String,
}

// 纯逻辑层的动作枚举：窗口层的 on_click 直接改写 AppState（同一套语义），
// 这里保留给 --sim / 自检复用。
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    ToggleDone(String),
    FlipMonth(i32),
    SelectDay(i32),
    AddCustom,
    DeleteRow(String),
    /// 进入 / 退出标签二内的归档视图
    ArchiveView(bool),
    /// 从归档恢复一条（id）
    ArchiveRestore(String),
    /// 从归档彻底删除一条（id）
    ArchivePurge(String),
    SwitchTab(Tab),
    HitOnly,
}

/// 日期胶囊展开后的 5 个选项（顺序即面板行序）
pub const NEWDATE_OPTIONS: [&str; 5] = ["今天", "明天", "本周五", "下周一", "自定义…"];

/// 「新建待办」的行内输入态：不是弹窗，而是插在列表顶部的一行临时行。
/// 提交后这行变正式行（进 `custom`），Esc / 空行回车都不创建。
#[derive(Clone, Debug)]
pub struct NewTodo {
    pub active: bool,
    pub text: String,
    pub focus: bool,
    /// 胶囊显示文案（默认「今天」）
    pub date_label: String,
    pub due: String,
    pub menu_open: bool,
    /// 菜单里高亮行（0..=4）；勾选行恒为当前值
    pub menu_sel: usize,
    /// 当前值在菜单里的下标（画勾用）
    pub date_sel: usize,
}

impl Default for NewTodo {
    fn default() -> Self {
        NewTodo {
            active: false,
            text: String::new(),
            focus: false,
            date_label: "今天".into(),
            due: String::new(),
            menu_open: false,
            menu_sel: 0,
            date_sel: 0,
        }
    }
}

impl NewTodo {
    /// 点「＋」：插入临时行、聚焦、日期默认今天
    pub fn open(&mut self, today: &str) {
        self.active = true;
        self.focus = true;
        self.text.clear();
        self.date_label = "今天".into();
        self.due = today.to_string();
        self.menu_open = false;
        self.menu_sel = 0;
        self.date_sel = 0;
    }

    /// Esc / 失焦取消：临时行整行消失
    pub fn cancel(&mut self) {
        *self = NewTodo::default();
    }

    /// 选日期（面板行 0..4；4 = 自定义，本预览里等价于保持当前值）
    pub fn pick_date(&mut self, idx: usize, today: &str, y: i32, mo: u32) {
        if idx >= NEWDATE_OPTIONS.len() {
            return;
        }
        self.date_sel = idx;
        self.date_label = NEWDATE_OPTIONS[idx].to_string();
        self.due = match idx {
            0 => today.to_string(),
            1 => add_days(today, 1),
            2 => next_weekday(today, 4),
            3 => next_weekday(today, 0),
            _ => add_days(today, 2),
        };
        let _ = (y, mo);
        self.menu_open = false;
        self.focus = true;
    }

    /// 回车提交：空行不创建。返回是否真的建了一条。
    pub fn commit(&mut self) -> bool {
        let t = self.text.trim();
        if t.is_empty() {
            return false;
        }
        *self = NewTodo::default();
        true
    }
}

/// 「＋ 新建待办」模态窗的表单态：整窗遮罩 + 中央面板，z 序最高。
/// 只存逻辑值，绘制在 `layout::draw_modal`。
#[derive(Clone, Debug)]
pub struct ModalForm {
    pub open: bool,
    /// 焦点字段：0=事项，1=备注
    pub field: usize,
    pub text: String,
    pub note: String,
    /// 备注每行的排版标记（与 note 的行一一对应）："" / "b" / "h1" / "h2" / "li" / "ol:N"，可用逗号组合
    pub note_style: Vec<String>,
    /// 备注光标（note 的字节下标）
    pub caret: usize,
    pub caret_t: usize,
    /// 事项框选区（字节区间）：支持拖选、Ctrl+A、Ctrl+C
    pub sel_t: Option<(usize, usize)>,
    /// 备注选区（字节区间 start<end）
    pub sel: Option<(usize, usize)>,
    /// ""（未选）/ "P0" / "P1" / "P2"
    pub prio: String,
    /// 已选图片路径（只存字符串，不渲染缩略图）
    pub img: String,
    /// YYYY-MM-DD
    pub due: String,
    /// 编辑既有待办时的 id（空 = 新建）
    pub edit_id: String,
    /// 到期胶囊下标（0..=4）；5=自定义；-1 = 未选
    pub due_sel: i32,
    /// 到期下拉菜单是否展开
    pub due_menu: bool,
    /// 「自定义」到期：在胶囊行内展开日期输入
    pub due_custom: bool,
    /// 自定义日期的编辑缓冲（YYYY-MM-DD）
    pub due_text: String,
    /// 自定义到期：日历选择器是否展开
    pub cal_open: bool,
    /// 日历当前年月
    pub cal_y: i32,
    pub cal_m: u32,
    /// 校验失败时的红字提示（空 = 无提示）
    pub err: String,
}

impl Default for ModalForm {
    fn default() -> Self {
        ModalForm {
            open: false,
            field: 0,
            text: String::new(),
            note: String::new(),
            note_style: Vec::new(),
            caret: 0,
            caret_t: 0,
            sel_t: None,
            sel: None,
            prio: String::new(),
            img: String::new(),
            due: String::new(),
            edit_id: String::new(),
            due_sel: -1,
            due_menu: false,
            due_custom: false,
            due_text: String::new(),
            cal_open: false,
            cal_y: 0,
            cal_m: 1,
            err: String::new(),
        }
    }
}

impl ModalForm {
    /// 到期胶囊的偏移天数：今天 / 明天 / 3 天 / 7 天 / 14 天
    pub const DUE_OFFSETS: [i64; 5] = [0, 1, 3, 7, 14];
    pub const DUE_LABELS: [&'static str; 7] = ["今天", "明天", "3 天", "7 天", "14 天", "自定义", "不设到期"];

    pub fn open(&mut self, today: &str) {
        *self = ModalForm {
            open: true,
            field: 0,
            text: String::new(),
            note: String::new(),
            note_style: Vec::new(),
            caret: 0,
            caret_t: 0,
            sel_t: None,
            sel: None,
            prio: String::new(),
            img: String::new(),
            due: today.to_string(),
            edit_id: String::new(),
            due_sel: 0,
            due_menu: false,
            due_custom: false,
            due_text: String::new(),
            cal_open: false,
            cal_y: 0,
            cal_m: 1,
            err: String::new(),
        };
    }

    /// 双击待办 → 编辑既有内容（日期/优先级/备注/附件都可改）
    pub fn open_edit(&mut self, id: &str, text: &str, note: &str, prio: &str, img: &str, due: &str) {
        self.open(due);
        self.edit_id = id.to_string();
        self.text = text.to_string();
        self.note = note.to_string();
        self.note_style = note.split('\n').map(|_| String::new()).collect();
        self.caret = self.note.len();
        self.caret_t = self.text.len();
        self.sel = None;
        self.prio = prio.to_string();
        self.img = img.to_string();
        self.due = due.to_string();
        self.due_text = due.to_string();
        self.due_sel = 5;
        self.due_custom = true;
    }

    /// 备注每行的排版标记（不足补空串）
    pub fn style_at(&self, line: usize) -> String {
        self.note_style
            .get(line)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_style(&mut self, line: usize, s: String) {
        if self.note_style.len() <= line {
            self.note_style.resize(line + 1, String::new());
        }
        self.note_style[line] = s;
    }

    /// 光标所在行号（按 '\n' 切）
    pub fn caret_line(&self) -> usize {
        self.note[..self.caret.min(self.note.len())]
            .chars()
            .filter(|c| *c == '\n')
            .count()
    }

    /// 当前行在 note 里的字节起点
    /// 当前行内容（不含换行符）
    /// 删事项框选区
    pub fn delete_sel_t(&mut self) -> bool {
        let Some((a, b)) = self.sel_t.take() else { return false; };
        let (a, b) = (a.min(self.text.len()), b.min(self.text.len()));
        if a >= b {
            self.caret_t = a;
            return false;
        }
        self.text.replace_range(a..b, "");
        self.caret_t = a;
        true
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.sel.take() else {
            return false;
        };
        let (a, b) = (a.min(self.note.len()), b.min(self.note.len()));
        if a >= b {
            return false;
        }
        self.note.replace_range(a..b, "");
        self.caret = a;
        true
    }

    pub fn move_caret(&mut self, delta: i32) -> bool {
        if let Some((a, b)) = self.sel.take() {
            self.caret = if delta < 0 { a } else { b };
            return true;
        }
        let mut idx = self.caret.min(self.note.len());
        if delta < 0 {
            if let Some((p, _)) = self.note[..idx].char_indices().last() {
                idx = p;
            }
        } else if idx < self.note.len() {
            idx += self.note[idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        }
        self.caret = idx;
        true
    }

    pub fn cancel(&mut self) {
        *self = ModalForm::default();
    }

    /// 优先级胶囊：只接受 P0/P1/P2，别的值视为清空
    pub fn set_prio(&mut self, p: &str) {
        self.prio = match p {
            "P0" | "P1" | "P2" => p.to_string(),
            _ => String::new(),
        };
    }

    /// 到期胶囊：due = today + DUE_OFFSETS[idx]
    pub fn set_due(&mut self, idx: usize, today: &str) {
        self.due_menu = false;
        if idx == 6 {
            // 不设到期：清空日期，行上不再显示倒计时
            self.due.clear();
            self.due_text.clear();
            self.due_sel = 6;
            self.due_custom = false;
            self.cal_open = false;
            self.field = 0;
            return;
        }
        if idx == 5 {
            self.due_sel = 5;
            self.due_custom = true;
            if self.due_text.is_empty() {
                self.due_text = self.due.clone();
            }
            self.field = 0;
            self.cal_open = true;
            if let Some((y, mo, _)) = parse_ymd(&self.due_text) {
                self.cal_y = y;
                self.cal_m = mo;
            } else if let Some((y, mo, _)) = parse_ymd(today) {
                self.cal_y = y;
                self.cal_m = mo;
            }
            return;
        }
        if idx >= Self::DUE_OFFSETS.len() {
            return;
        }
        self.due_sel = idx as i32;
        self.due_custom = false;
        self.cal_open = false;
        self.field = 0;
        self.due = add_days(today, Self::DUE_OFFSETS[idx]);
        self.due_text = self.due.clone();
    }

    pub fn due_label(&self) -> String {
        if self.due_custom {
            // 自定义日期被清空（例如在日历里点了「不设到期」）时，胶囊不能是空白
            if self.due_text.trim().is_empty() {
                return "不设到期".to_string();
            }
            return self.due_text.clone();
        }
        if self.due_sel >= 0 && (self.due_sel as usize) < Self::DUE_LABELS.len() {
            return Self::DUE_LABELS[self.due_sel as usize].to_string();
        }
        "选择日期".to_string()
    }

    /// Tab / Shift+Tab：两个字段来回切（只有两个字段，方向等价）
    pub fn tab(&mut self, back: bool) {
        let n = 2;
        self.field = if back { (self.field + n - 1) % n } else { (self.field + 1) % n };
    }

    /// 提交自定义日期；非法时返回 false（不改 due）
    pub fn commit_custom(&mut self) -> bool {
        if !self.due_custom {
            return true;
        }
        let t = self.due_text.trim();
        // 允许「不设到期」：没有日期也视为合法，保存为空 due
        if t.is_empty() {
            self.due.clear();
            self.due_text.clear();
            self.due_sel = 6;
            return true;
        }
        let ok = if t.len() == 10 && t.as_bytes()[4] == b'-' && t.as_bytes()[7] == b'-' {
            match parse_ymd(t) {
                Some((y, m, d)) => (2000..=2100).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d),
                None => false,
            }
        } else {
            false
        };
        if ok {
            self.due = t.to_string();
        }
        ok
    }

    /// 日历：月份加减
    pub fn cal_move(&mut self, delta: i32) {
        let mut m = self.cal_m as i32 + delta;
        let mut y = self.cal_y;
        while m < 1 {
            m += 12;
            y -= 1;
        }
        while m > 12 {
            m -= 12;
            y += 1;
        }
        self.cal_y = y;
        self.cal_m = m as u32;
    }

    /// 日历：点某一天，写回 due
    pub fn cal_pick(&mut self, day: u32) {
        self.due = format!("{:04}-{:02}-{:02}", self.cal_y, self.cal_m, day);
        self.due_text = self.due.clone();
        self.due_custom = true;
        self.due_sel = 4;
        self.cal_open = false;
        self.field = 0;
    }

    pub fn cal_close(&mut self) {
        self.cal_open = false;
    }

    /// 日历网格：返回 (周一起始列 0..6, 当月天数, 上/下月余量按需要)
    pub fn cal_grid(&self) -> (u32, u32) {
        let dim = lunar::days_in_month(self.cal_y, self.cal_m);
        let first = lunar::days_from_civil(self.cal_y, self.cal_m, 1) as i64;
        let lead = ((first + 3).rem_euclid(7)) as u32;
        (lead, dim)
    }

    pub fn backspace(&mut self) {
        match self.field {
            0 => {
                if self.delete_sel_t() { return; }
                let idx = self.caret_t.min(self.text.len());
                if let Some((p, _)) = self.text[..idx].char_indices().last() {
                    self.text.replace_range(p..idx, "");
                    self.caret_t = p;
                }
            }
            1 => {
                if !self.delete_selection() {
                    let idx = self.caret.min(self.note.len());
                    if let Some((p, _)) = self.note[..idx].char_indices().last() {
                        self.note.replace_range(p..idx, "");
                        self.caret = p;
                    }
                }
            }
            _ => {
                self.due_text.pop();
            }
        }
    }

    /// 事项单行框光标左右移动（按字符边界）
    pub fn move_caret_t(&mut self, delta: i32) -> bool {
        let len = self.text.len();
        let idx = self.caret_t.min(len);
        if delta < 0 {
            if idx == 0 { return false; }
            let p = self.text[..idx].char_indices().last().map(|(p, _)| p).unwrap_or(0);
            self.caret_t = p;
        } else {
            if idx >= len { return false; }
            let n = self.text[idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
            self.caret_t = idx + n;
        }
        true
    }

    /// 备注里回车：插入换行；列表行自动续行（项目符号沿用、编号 +1）
    pub fn note_newline(&mut self) -> bool {
        if self.note.chars().count() >= 400 {
            return false;
        }
        self.delete_selection();
        let line = self.caret_line();
        let style = self.style_at(line);
        let mut next_style = String::new();
        if style.split(',').any(|f| f == "li") {
            next_style.push_str("li");
        } else if let Some(n) = style
            .split(',')
            .find_map(|f| f.strip_prefix("ol:"))
            .and_then(|v| v.parse::<u32>().ok())
        {
            next_style.push_str(&format!("ol:{}", n + 1));
        }
        let idx = self.caret.min(self.note.len());
        self.note.insert(idx, '\n');
        self.caret = idx + 1;
        // 新行样式写在「换行后」的那一行
        let new_line = self.caret_line();
        self.set_style(new_line, next_style);
        true
    }

    /// 上屏字符进当前字段；到上限返回 false（不重绘）
    pub fn push_char(&mut self, ch: char) -> bool {
        if self.field == 1 {
            if self.note.chars().count() >= 400 {
                return false;
            }
            self.delete_selection();
            let idx = self.caret.min(self.note.len());
            self.note.insert(idx, ch);
            self.caret = idx + ch.len_utf8();
            return true;
        }
        if self.field == 0 {
            self.delete_sel_t();
            if self.text.chars().count() >= 60 || ch == '\n' {
                return false;
            }
            let idx = self.caret_t.min(self.text.len());
            self.text.insert(idx, ch);
            self.caret_t = idx + ch.len_utf8();
            return true;
        }
        let (slot, max) = match self.field {
            0 => (&mut self.text, 60usize),
            _ => {
                if !ch.is_ascii_digit() && ch != '-' {
                    return false;
                }
                if self.due_text.chars().count() >= 10 {
                    return false;
                }
                (&mut self.due_text, 10usize)
            }
        };
        if slot.chars().count() >= max {
            return false;
        }
        if ch == '\n' {
            return false;
        }
        slot.push(ch);
        true
    }
}

/// `YYYY-MM-DD` + n 天（只用于预览的日期胶囊演示，不做跨时区处理）
pub fn add_days(iso: &str, n: i64) -> String {
    let (y, m, d) = match parse_ymd(iso) {
        Some(v) => v,
        None => return iso.to_string(),
    };
    let mut days = lunar::days_from_civil(y, m, d as u32) as i64 + n;
    let (mut yy, mut mm) = (y, m);
    loop {
        let dim = lunar::days_in_month(yy, mm) as i64;
        let first = lunar::days_from_civil(yy, mm, 1) as i64;
        if days < first {
            mm -= 1;
            if mm == 0 {
                mm = 12;
                yy -= 1;
            }
            continue;
        }
        if days >= first + dim {
            days -= dim;
            mm += 1;
            if mm == 13 {
                mm = 1;
                yy += 1;
            }
            continue;
        }
        return format!("{:04}-{:02}-{:02}", yy, mm, days - first + 1);
    }
}

/// 下一个指定星期（0=周一 … 6=周日）的日期；今天就是则算下周
fn next_weekday(iso: &str, wd: i64) -> String {
    let (y, m, d) = match parse_ymd(iso) {
        Some(v) => v,
        None => return iso.to_string(),
    };
    let cur = (lunar::weekday(y, m, d as u32) as i64 + 6) % 7;
    let mut delta = (wd - cur + 7) % 7;
    if delta == 0 {
        delta = 7;
    }
    add_days(iso, delta)
}

/// 主题底色预设（顺序与设置面板一致）
pub const TINTS: [(&str, (u8, u8, u8)); 6] = [
    ("#151A26", (21, 26, 38)),   // 夜蓝
    ("#121F1A", (18, 31, 26)),   // 墨绿
    ("#1E1524", (30, 21, 36)),   // 紫檀
    ("#241A12", (36, 26, 18)),   // 咖
    ("#0D0F14", (13, 15, 20)),   // 石墨
    ("#141C22", (20, 28, 34)),   // 青灰
];

/// 挂件尺寸预设：小 / 中 / 大
pub const SIZES: [(i32, i32); 3] = [(820, 474), (980, 566), (1140, 658)];

/// 设置面板的临时编辑态；点保存才写回 state.ui
#[derive(Debug, Clone)]
pub struct SetForm {
    pub open: bool,
    pub opacity: f64,
    pub tint: usize,
    pub size: usize,
    pub locked: bool,
}

impl Default for SetForm {
    fn default() -> Self {
        SetForm { open: false, opacity: 1.0, tint: 0, size: 1, locked: false }
    }
}

pub struct View<'a> {
    pub d: &'a Data,
    pub tab: Tab,
    pub y: i32,
    pub mo: u32,
    /// 勾选状态，key 与 WPF 版一致："model|name|d"；部门项 key = "dept|text|due"
    pub done: BTreeMap<String, bool>,
    pub dept: Vec<DeptRow>,
    pub custom: Vec<DeptRow>,
    pub sel_day: i32,
    pub signals: Vec<SignalRow>,
    pub signals_sample: bool,
    pub locked: bool,
    /// 删除二次确认：命中的按钮 id（"dept:2:del"），3 秒内再点才真删。
    pub pending_del: Option<String>,
    /// 标签二内是否切到归档视图（不新增第三标签）
    pub arc_mode: bool,
    pub archive: Vec<ArcRow>,
    /// 归档列表滚动位置（行）
    pub arc_scroll: usize,
    /// 归档「彻底删除」二次确认的按钮 id（"arc:2:purge"）
    pub arc_pending: Option<String>,
    /// 新建待办：列表顶部的临时输入行（不是弹窗）
    pub newtodo: NewTodo,
    /// 新建待办模态窗（＋ 现在走这个）
    pub modal: ModalForm,
        /// 鼠标位置（客户区坐标）；(-1,-1) = 不在窗口内
    pub mouse: (i32, i32),
    /// 点日历某天后，左卡切换成「当天节点」列表（空 = 不切换）
    pub day_list: Vec<DeptRow>,
    pub day_label: String,
    /// 待办列表滚动起始行（超出 9 行时用滚轮）
    pub scroll: usize,
    /// 设置面板
    pub set: SetForm,
    /// 行拖拽排序中：(组 0=custom/1=dept, 起始下标, 相对按下的 dy, 目标下标)
    pub drag: Option<(usize, i32, usize)>,
    /// E-Mobile 联通状态：0 未知 / 1 绿 / 2 黄 / 3 红
    pub emstat: u8,
    /// 状态灯说明浮层是否展开
    pub em_tip: bool,
    /// 会议安排：三档数据 + 当前档位（0 当日 / 1 当周 / 2 当月）+ 抓取时间
    pub meet_today: Vec<MeetRow>,
    pub meet_week: Vec<MeetRow>,
    pub meet_month: Vec<MeetRow>,
    pub meet_span: u8,
    pub meet_at: String,
    /// 开会前 5 分钟的强制提醒（None = 无）
    pub remind: Option<Remind>,
}

impl<'a> View<'a> {
    pub fn new(d: &'a Data) -> Self {
        let mut v = View {
            d,
            tab: Tab::Proj,
            y: if d.cal.y != 0 { d.cal.y } else { 2026 },
            mo: if d.cal.mo > 0 { d.cal.mo as u32 } else { 1 },
            done: BTreeMap::new(),
            dept: Vec::new(),
            custom: Vec::new(),
            sel_day: 0,
            signals: Vec::new(),
            signals_sample: false,
            locked: false,
            pending_del: None,
            arc_mode: false,
            archive: Vec::new(),
            arc_scroll: 0,
            day_list: Vec::new(),
            day_label: String::new(),
            arc_pending: None,
            newtodo: NewTodo::default(),
            modal: ModalForm::default(),
            mouse: (-1, -1),
            set: SetForm::default(),
            scroll: 0,
            drag: None,
            emstat: 0,
            em_tip: false,
            meet_today: Vec::new(),
            meet_week: Vec::new(),
            meet_month: Vec::new(),
            meet_span: 0,
            meet_at: String::new(),
            remind: None,
        };
        v.load_signals();
        v
    }

    fn load_signals(&mut self) {
        self.signals_sample = false;
        self.signals = self
            .d
            .signals
            .iter()
            .take(3)
            .map(|s: &DataSignal| SignalRow {
                t: s.t.clone(),
                sender: s.sender.clone(),
                text: s.text.clone(),
                src: s.src.clone(),
                kind: s.kind.clone(),
                conv: s.conv.clone(),
            })
            .collect();
    }

    pub fn days_in(&self) -> u32 {
        lunar::days_in_month(self.y, self.mo)
    }

    /// 周一为 0 的起始补白。
    pub fn start_pad(&self) -> i32 {
        ((lunar::weekday(self.y, self.mo, 1) + 6) % 7) as i32
    }

    pub fn iso(&self, day: i32) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.mo, day)
    }

    pub fn key_of_todo(&self, i: usize) -> String {
        match self.d.todo.get(i) {
            Some(t) => format!("{}|{}|{}", t.model, t.name, t.d),
            None => String::new(),
        }
    }

    pub fn key_of_dept(&self, i: usize) -> String {
        match self.dept.get(i) {
            Some(r) => format!("dept|{}|{}", r.text, r.due),
            None => String::new(),
        }
    }

    pub fn key_of_custom(&self, i: usize) -> String {
        match self.custom.get(i) {
            Some(r) => format!("custom|{}|{}", r.text, r.due),
            None => String::new(),
        }
    }

    #[allow(dead_code)]
    pub fn toggle(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        let cur = self.done.get(key).copied().unwrap_or(false);
        if cur {
            self.done.remove(key);
        } else {
            self.done.insert(key.to_string(), true);
        }
        // 部门项勾选要写回 department[].done（不复制到 custom）——t3 由 state 层落盘，
        // 这里只改内存镜像。
        for r in self.dept.iter_mut() {
            if format!("dept|{}|{}", r.text, r.due) == key {
                r.done = !cur;
            }
        }
        for r in self.custom.iter_mut() {
            if format!("custom|{}|{}", r.text, r.due) == key {
                r.done = !cur;
            }
        }
    }

    pub fn is_done(&self, key: &str) -> bool {
        self.done.get(key).copied().unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn flip(&mut self, delta: i32) {
        let mut m = self.mo as i32 + delta;
        let mut y = self.y;
        while m < 1 {
            m += 12;
            y -= 1;
        }
        while m > 12 {
            m -= 12;
            y += 1;
        }
        self.mo = m as u32;
        self.y = y;
        self.sel_day = 0;
    }

    #[allow(dead_code)]
    pub fn switch_tab(&mut self, t: Tab) {
        if self.tab != t {
            self.tab = t;
            self.sel_day = 0;
        }
    }

    /// 归档视图滚轮：一次 3 行，保证外层矩形不动。
    pub fn arc_wheel(&mut self, up: bool) -> bool {
        if !self.arc_mode {
            return false;
        }
        let rows = crate::layout::L_ROWS_ARC;
        let max = self.archive.len().saturating_sub(rows);
        let step = 3usize;
        let before = self.arc_scroll;
        if up {
            self.arc_scroll = self.arc_scroll.saturating_sub(step);
        } else {
            self.arc_scroll = (self.arc_scroll + step).min(max);
        }
        self.arc_scroll != before
    }

    /// 距到期还有几天（负=逾期），用于倒计时上色
    pub fn days_left(&self, due: &str) -> Option<i64> {
        let (dy, dm, dd) = crate::view::parse_ymd(due)?;
        let (ty, tm, td) = crate::view::parse_ymd(&crate::log::now_date())?;
        Some(days_from_civil(dy, dm as i32, dd) - days_from_civil(ty, tm as i32, td))
    }

    /// 待办倒计时：「逾期N」/「今天」/「明天」/「N天」
    pub fn countdown(&self, due: &str) -> String {
        let (dy, dm, dd) = match crate::view::parse_ymd(due) {
            Some(x) => x,
            None => return String::new(),
        };
        let (ty, tm, td) = match crate::view::parse_ymd(&crate::log::now_date()) {
            Some(x) => x,
            None => return String::new(),
        };
        let n = days_from_civil(dy, dm as i32, dd) - days_from_civil(ty, tm as i32, td);
        if n < 0 {
            format!("逾期{}", -n)
        } else if n == 0 {
            "今天".to_string()
        } else if n == 1 {
            "明天".to_string()
        } else {
            format!("{}天", n)
        }
    }

    pub fn today_day(&self) -> i32 {
        if let Some((y, m, d)) = crate::view::parse_ymd(&self.d.today) {
            if y == self.y && m == self.mo {
                return d;
            }
        }
        -1
    }

    /// 日期格里的节点点颜色：标签一取项目节点，标签二取待办 due 日。
    pub fn day_dots(&self, day: i32) -> Vec<Rgba> {
        let iso = self.iso(day);
        match self.tab {
            Tab::Meet => {
                // 会议：与左侧列表同源（当日/当周/当月合并去重），琥珀色小点
                let mut out: Vec<Rgba> = Vec::new();
                let mut seen: Vec<(String, String)> = Vec::new();
                for r in self
                    .meet_today
                    .iter()
                    .chain(self.meet_week.iter())
                    .chain(self.meet_month.iter())
                {
                    if r.date != iso {
                        continue;
                    }
                    let key = (r.start.clone(), r.name.clone());
                    if seen.contains(&key) {
                        continue;
                    }
                    seen.push(key);
                    out.push((255, 194, 71, 235));
                }
                out
            }
            Tab::Proj => self
                .d
                .cal
                .per_day
                .get(&iso)
                .map(|v| v.iter().map(|p| prio_str(p)).collect())
                .unwrap_or_default(),
            Tab::Dept => {
                let mut out = Vec::new();
                for r in self.dept.iter().chain(self.custom.iter()) {
                    if r.due == iso {
                        // 紧急程度直接映到颜色：P0 红 / P1 橙 / P2 蓝 / 未标灰
                        out.push(if r.done {
                            (111, 118, 134, 255)
                        } else {
                            match r.prio.as_str() {
                                "P0" => P0C,
                                "P1" => P1C,
                                "P2" => P2C,
                                _ => (139, 147, 164, 255),
                            }
                        });
                    }
                }
                out
            }
        }
    }

    /// 会议安排当前档位的行。
    pub fn meet_rows(&self) -> &Vec<MeetRow> {
        match self.meet_span {
            1 => &self.meet_week,
            2 => &self.meet_month,
            _ => &self.meet_today,
        }
    }

    /// 标签二的列表：部门导入在前、自建在后。
    pub fn dept_rows(&self) -> usize {
        self.dept.len() + self.custom.len()
    }

    #[allow(dead_code)]
    pub fn on_hit(&mut self, h: &Hit) -> Action {
        if h.kind == Kind::Chip && (h.id == "tab.proj" || h.id == "tab.dept") {
            let t = if h.id == "tab.dept" { Tab::Dept } else { Tab::Proj };
            if self.tab != t {
                self.switch_tab(t);
                return Action::SwitchTab(t);
            }
            return Action::HitOnly;
        }
        if self.locked {
            return Action::None;
        }
        match h.kind {
            Kind::Circle => {
                let id = h.id.as_str();
                if let Some(idx) = id.strip_prefix("todo:").and_then(|s| s.parse::<usize>().ok()) {
                    let k = self.key_of_todo(idx);
                    self.toggle(&k);
                    return Action::ToggleDone(k);
                }
                if let Some(idx) = id.strip_prefix("dept:").and_then(|s| s.parse::<usize>().ok()) {
                    let k = self.key_of_dept(idx);
                    self.toggle(&k);
                    return Action::ToggleDone(k);
                }
                if let Some(idx) = id.strip_prefix("mine:").and_then(|s| s.parse::<usize>().ok()) {
                    let k = self.key_of_custom(idx);
                    self.toggle(&k);
                    return Action::ToggleDone(k);
                }
                Action::HitOnly
            }
            Kind::MonthPrev => {
                self.flip(-1);
                Action::FlipMonth(-1)
            }
            Kind::MonthNext => {
                self.flip(1);
                Action::FlipMonth(1)
            }
            Kind::DayCell => {
                if let Some(day) = h
                    .id
                    .strip_prefix("day:")
                    .and_then(|s| s.rsplit('-').next().and_then(|d| d.parse::<i32>().ok()))
                {
                    self.sel_day = day;
                    return Action::SelectDay(day);
                }
                Action::HitOnly
            }
            Kind::Button => {
                let id = h.id.as_str();
                // 归档视图：进入 / 返回 / 恢复 / 彻底删除
                if id == "arc:open" {
                    self.arc_mode = true;
                    self.arc_scroll = 0;
                    return Action::ArchiveView(true);
                }
                if id == "arc:back" {
                    self.arc_mode = false;
                    self.arc_pending = None;
                    return Action::ArchiveView(false);
                }
                if let Some(rest) = id.strip_prefix("arc:") {
                    if let Some((i_s, verb)) = rest.split_once(':') {
                        if let Ok(i) = i_s.parse::<usize>() {
                            if let Some(a) = self.archive.get(i) {
                                let aid = a.id.clone();
                                match verb {
                                    "restore" => return Action::ArchiveRestore(aid),
                                    "purge" => return Action::ArchivePurge(aid),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if id == "mine:add" {
                    return Action::AddCustom;
                }
                // 删除按钮：dept:{i}:del / mine:{i}:del
                if let Some(rest) = id.strip_suffix(":del") {
                    if let Some((prefix, idxs)) = rest.split_once(':') {
                        if let Ok(idx) = idxs.parse::<usize>() {
                            if prefix == "dept" && idx < self.dept.len() {
                                let k = self.key_of_dept(idx);
                                self.done.remove(&k);
                                self.dept.remove(idx);
                            } else if prefix == "mine" && idx < self.custom.len() {
                                let k = self.key_of_custom(idx);
                                self.done.remove(&k);
                                self.custom.remove(idx);
                            }
                            return Action::DeleteRow(id.to_string());
                        }
                    }
                }
                Action::HitOnly
            }
            _ => Action::HitOnly,
        }
    }
}

/// 数据层 per_day 里存的是 "P0"/"P1"/"P2" 字符串。
fn prio_str(p: &str) -> Rgba {
    match p {
        "P0" => P0C,
        "P1" => P1C,
        "P2" => P2C,
        _ => (111, 118, 134, 255),
    }
}

pub fn parse_ymd(s: &str) -> Option<(i32, u32, i32)> {
    let mut it = s.split('-');
    let y = it.next()?.parse::<i32>().ok()?;
    let m = it.next()?.parse::<u32>().ok()?;
    let d = it.next()?.parse::<i32>().ok()?;
    Some((y, m, d))
}

/// 公历日期 → 连续日序（Howard Hinnant 算法）
pub fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
