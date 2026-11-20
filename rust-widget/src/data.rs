// 数据层：widget_data.json（build_widget.mjs 生成）
// 约定：未知字段忽略；缺字段取默认值；任何解析失败都不崩，退回内置空数据并记日志。
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::log::Logger;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Counts {
    pub projects: i32,
    pub p0: i32,
    #[serde(rename = "nodes10")]
    pub nodes10: i32,
    #[serde(rename = "nodes7")]
    pub nodes7: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TodoItem {
    pub model: String,
    pub name: String,
    pub prio: String,
    pub dd: i32,
    pub d: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct P0Item {
    pub model: String,
    pub blocker: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Node {
    pub d: String,
    pub dd: i32,
    pub model: String,
    pub prio: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cal {
    pub y: i32,
    pub mo: i32,
    pub start_pad: i32,
    pub days_in: i32,
    /// "2026-11-10" -> ["P0", "P1", ...]
    pub per_day: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Signal {
    /// 时间 "2026-11-10 14:01"
    pub t: String,
    /// 发送人；个人会话追加 "·私聊"
    pub sender: String,
    /// 一句话原文（去换行、截断 60 字）
    pub text: String,
    /// 关联型号，可为空串
    pub model: String,
    /// 数据来源："EM"（E-Mobile）/"企微"（企微本地库），空串=不显示角标
    pub src: String,
    /// 会话类型："group" | "direct"，空串=未知
    pub kind: String,
    /// 会话（群）名称，用于区分同名发言人
    pub conv: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WidgetData {
    pub today: String,
    pub today_day: i32,
    pub week: String,
    pub ym: String,
    pub stamp: String,
    pub counts: Counts,
    pub todo: Vec<TodoItem>,
    pub p0: Vec<P0Item>,
    pub nodes: Vec<Node>,
    /// 今日高相关群消息，最多 3 条（展示层再截断，解析层全收）
    pub signals: Vec<Signal>,
    /// "unread" | "empty"：空态由上游明确给出，不靠 signals.len() 猜
    pub signals_mode: String,
    pub health: Health,
    pub cal: Cal,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Health {
    pub emobile: Emobile,
    pub alerts: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Emobile {
    pub host: String,
    pub port: u16,
    pub at: String,
    pub age_min: i64,
    pub msgs: i64,
    pub live: bool,
    pub live_msgs: i64,
    pub live_err: String,
    /// 抓取周期（分钟，白天/夜间取当前时段），0 表示未知
    pub interval_min: i64,
    /// 上一轮管道是否整体成功（brief_run_status.steps 全 ok）
    pub run_ok: bool,
    /// E-Mobile 令牌是否可用（build_widget 写入；老数据缺省）
    pub auth_ok: bool,
    /// 令牌校验时间（ISO，本地时区）
    pub auth_at: String,
    /// SCRAM 握手是否成功
    pub scram_ok: bool,
}

impl WidgetData {
    /// 一行摘要，既写日志也用于验收计数（nodes=63 todo=5 p0=4）。
    pub fn summary(&self) -> String {
        format!(
            "todo={} p0={} nodes={} signals={} perDay={} counts=projects:{}/p0:{}/nodes10:{}/nodes7:{} today={} todayDay={} week={} ym={} stamp={}",
            self.todo.len(),
            self.p0.len(),
            self.nodes.len(),
            self.signals.len(),
            self.cal.per_day.len(),
            self.counts.projects,
            self.counts.p0,
            self.counts.nodes10,
            self.counts.nodes7,
            self.today,
            self.today_day,
            self.week,
            self.ym,
            self.stamp
        )
    }

    /// 顶部「项目进度」信号条：最多 3 条。t3 渲染时使用。
    #[allow(dead_code)]
    pub fn signals_top(&self, n: usize) -> &[Signal] {
        let end = self.signals.len().min(n);
        &self.signals[..end]
    }

    /// 某天（"YYYY-MM-DD"）的优先级标记；无数据返回空切片。t3 画月历时使用。
    #[allow(dead_code)]
    pub fn day_prios(&self, iso: &str) -> &[String] {
        self.cal
            .per_day
            .get(iso)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// 读取并解析数据文件；失败时记日志并返回空数据（进程继续跑）。
/// 返回 (data, 是否成功解析)。
pub fn load(path: &Path, log: &Logger) -> (WidgetData, bool) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            log.line(&format!(
                "[data] read failed path={} err={} -> empty dataset",
                path.display(),
                e
            ));
            return (WidgetData::default(), false);
        }
    };
    match serde_json::from_str::<WidgetData>(&text) {
        Ok(d) => {
            log.line(&format!("[data] loaded {} bytes {}", text.len(), d.summary()));
            (d, true)
        }
        Err(e) => {
            log.line(&format!(
                "[data] parse failed path={} err={} -> empty dataset (process stays alive)",
                path.display(),
                e
            ));
            (WidgetData::default(), false)
        }
    }
}

/// 文件 mtime，用于 30 秒轮询的变更检测。
pub fn mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}
