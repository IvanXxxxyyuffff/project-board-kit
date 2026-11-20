//! 命中链：整数矩形数组 + z 序功能异常（后压入=在上层，反向扫描第一个命中即结果）。
//! 禁止逐像素 alpha 判定：预乘缓冲的 alpha 与屏幕实际覆盖率不能一一对应，而且慢。
//! 只有 `DragBlank` / `DragCalNonDay` 允许拖动窗口；其余一律 arrow 光标 + 各自动作。

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// 面板空白（可拖窗）
    DragBlank,
    /// 卡片正文空白：不拖窗、不响应点击（点列表空白不该把窗口拽走）
    Static,
    /// 月历非日期格（表头带、上下补白格、网格空白）
    DragCalNonDay,
    /// 日期格：不拖，点击下钻
    DayCell,
    /// 勾选圈
    Circle,
    /// 优先级胶囊
    Tag,
    /// 正文文字
    Text,
    /// 整行（行尾天数、行内空白）
    Row,
    /// 按钮（＋ / 添加一条）
    Button,
    MonthPrev,
    MonthNext,
    /// 头部计数胶囊
    Chip,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::DragBlank => "drag_blank",
            Kind::Static => "static",
            Kind::DragCalNonDay => "drag_cal_nonday",
            Kind::DayCell => "day_cell",
            Kind::Circle => "circle",
            Kind::Tag => "tag",
            Kind::Text => "text",
            Kind::Row => "row",
            Kind::Button => "button",
            Kind::MonthPrev => "month_prev",
            Kind::MonthNext => "month_next",
            Kind::Chip => "chip",
        }
    }

    pub fn draggable(self) -> bool {
        matches!(self, Kind::DragBlank | Kind::DragCalNonDay)
    }

    /// 双击可打开项目看板的目标：项目卡内正文 / 项目行 / 日期格 / 优先级胶囊。
    /// 勾选圈、按钮、标签页胶囊、翻月箭头、空白与拖拽区一律不算（双击仍等于两次单击）。
    pub fn openable(self) -> bool {
        matches!(self, Kind::Text | Kind::Row | Kind::DayCell | Kind::Tag)
    }

    /// 验收口径 A12：空白 sizeall、控件 arrow。锁定态由调用方统一改成 arrow。
    pub fn cursor(self) -> &'static str {
        if self.draggable() {
            "sizeall"
        } else {
            "arrow"
        }
    }
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub kind: Kind,
}

impl Hit {
    #[inline]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    #[allow(dead_code)]
    pub fn cx(&self) -> i32 {
        self.x + self.w / 2
    }
    #[allow(dead_code)]
    pub fn cy(&self) -> i32 {
        self.y + self.h / 2
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\"id\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"kind\":\"{}\",\"cursor\":\"{}\",\"drag\":{}}}",
            esc(&self.id),
            self.x,
            self.y,
            self.w,
            self.h,
            self.kind.name(),
            self.kind.cursor(),
            self.kind.draggable()
        )
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

pub struct Hits {
    pub v: Vec<Hit>,
}

impl Hits {
    pub fn new() -> Self {
        Hits { v: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.v.clear();
    }

    pub fn push(&mut self, kind: Kind, id: impl Into<String>, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.v.push(Hit { id: id.into(), x, y, w, h, kind });
    }

    /// z 序功能异常：反向扫描，第一个命中即返回。整屏被 panel 的 DragBlank 兜底，永不落空。
    pub fn hit(&self, x: i32, y: i32) -> Option<&Hit> {
        self.v.iter().rev().find(|h| h.contains(x, y))
    }

    /// 只保留矩形外的命中区（脏区重画前调用，避免控件命中区重复累积）。
    /// 保留区域外的命中区，但区域自身的容器命中（同 x/y/w/h）保留。
    /// 卡片拖拽区就是在脏区重画时既不重复登记、也不被裁掉的特殊一条。
    pub fn retain_outside_keep_container(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.v.retain(|t| {
            let is_container = t.x == x && t.y == y && t.w == w && t.h == h;
            is_container || !(t.x >= x && t.y >= y && t.x < x + w && t.y < y + h)
        });
    }

    #[allow(dead_code)]
    pub fn retain_outside(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.v.retain(|t| {
            !(t.x >= x && t.y >= y && t.x < x + w && t.y < y + h)
        });
    }

    #[allow(dead_code)]
    pub fn find_id(&self, id: &str) -> Option<&Hit> {
        self.v.iter().find(|h| h.id == id)
    }

    #[allow(dead_code)]
    pub fn first_kind(&self, k: Kind) -> Option<&Hit> {
        self.v.iter().find(|h| h.kind == k)
    }

    pub fn json(&self) -> String {
        let mut s = String::from("[");
        for (i, h) in self.v.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&h.to_json());
        }
        s.push(']');
        s
    }

    #[allow(dead_code)]
    pub fn dump(&self, path: &str) -> std::io::Result<()> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.json())
    }

    /// 按 kind 统计，日志/交付里报一行就够。
    pub fn summary(&self) -> String {
        let mut blank = 0;
        let mut calnd = 0;
        let mut cells = 0;
        let mut ctrl = 0;
        for h in &self.v {
            match h.kind {
                Kind::DragBlank => blank += 1,
                Kind::DragCalNonDay => calnd += 1,
                Kind::DayCell => cells += 1,
                _ => ctrl += 1,
            }
        }
        format!(
            "regions={} blank={} cal_nonday={} day_cell={} controls={}",
            self.v.len(),
            blank,
            calnd,
            cells,
            ctrl
        )
    }
}
