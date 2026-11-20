// 状态层：widget_state.json —— 与现有 WPF 版共用同一文件。
// 结构：
//   { "custom":[{id,text,due,done}], "department":[{id,text,due,done,src,importedAt}],
//     "done":{key:true}, "ui":{tab,win_mode,x,y,opacity} }
// 原则：
//   1) 整文件读入 → 修改 → 写回，不重建对象，未知字段原样保留（ui 段由 UI 读写，数据层不动）；
//   2) 老文件只有 custom/done 时自动补空 department 与默认 ui，不丢数据；
//   3) department 首次从 yynote_todos.json 导入；已存在则按 id 合并、本地 done 优先；
//   4) 写入先写 .tmp 再 rename（原子），UTF-8 无 BOM；
//   5) 解析失败：把原文件改名 widget_state.json.bad-<ts> 备份，重建空结构，错误记 widget_state_error.log。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::log::{now_str, Logger};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomItem {
    pub id: String,
    pub text: String,
    pub due: String,
    pub done: bool,
    /// P0 / P1 / P2（空=未标）
    pub prio: String,
    /// 备注（单行，可空）
    pub note: String,
    /// 图片路径（可空，本地文件）
    pub img: String,
    /// WPF 版可能追加的字段：不解析也不丢。
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DepartmentItem {
    /// P0 / P1 / P2（空=未标）
    pub prio: String,
    /// 备注（可空）
    pub note: String,
    pub id: String,
    pub text: String,
    /// 原 yynote 待办可能没有截止日 → null
    pub due: Option<String>,
    pub done: bool,
    /// "yynote" = 导入项；空串 = 用户自建
    pub src: String,
    #[serde(rename = "importedAt")]
    pub imported_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ArchiveItem {
    #[serde(default)]
    pub prio: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub img: String,
    pub id: String,
    pub text: String,
    pub due: Option<String>,
    pub done: bool,
    /// "yynote" | "user" —— 决定恢复时回到哪个数组
    pub src: String,
    /// ISO 8601 带本地时区，如 2026-11-10T14:40:12+08:00
    #[serde(rename = "archivedAt")]
    pub archived_at: String,
    /// 仅 "done" | "deleted"
    pub reason: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ArchiveItem {
    pub fn is_valid_reason(s: &str) -> bool {
        s == "done" || s == "deleted"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    /// "proj" | "dept"
    pub tab: String,
    /// "embed" | "float"
    #[serde(rename = "win_mode")]
    pub win_mode: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub opacity: f64,
    /// 主题底色 #RRGGBB
    pub tint: String,
    /// 挂件尺寸（逻辑像素）
    pub win_w: i32,
    pub win_h: i32,
    /// 固定位置：开启后不可拖动
    pub locked: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tab: "proj".to_string(),
            win_mode: "embed".to_string(),
            x: None,
            y: None,
            opacity: 1.0,
            tint: "#12141A".to_string(),
            win_w: 980,
            win_h: 566,
            locked: false,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub custom: Vec<CustomItem>,
    pub department: Vec<DepartmentItem>,
    /// 勾选状态：key -> true。key 约定 "model|name|d"，缺失即未勾选。
    pub done: BTreeMap<String, bool>,
    /// 归档：软删除 / 手动归档已完成的待办，只在本文件留痕，不物理删。
    pub archive: Vec<ArchiveItem>,
    pub ui: UiState,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AppState {
    pub fn is_done(&self, key: &str) -> bool {
        self.done.get(key).copied().unwrap_or(false)
    }

    pub fn set_done(&mut self, key: &str, value: bool) {
        if value {
            self.done.insert(key.to_string(), true);
        } else {
            self.done.remove(key);
        }
    }

    /// 勾选/取消切换，返回切换后的状态。t3 点击条目时使用。
    #[allow(dead_code)]
    pub fn toggle(&mut self, key: &str) -> bool {
        let next = !self.is_done(key);
        self.set_done(key, next);
        next
    }

    pub fn add_custom(&mut self, text: &str, due: &str) {
        self.add_custom_full(text, due, "", "", "");
    }

    /// 门户窗（＋ 新建待办）用：一次写全 text / due / prio / note / img。
    /// 空正文不创建，返回新条目的 id（未创建返回 None）。
    pub fn add_custom_full(
        &mut self,
        text: &str,
        due: &str,
        prio: &str,
        note: &str,
        img: &str,
    ) -> Option<String> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let id = format!("c{}", compact_ts());
        self.custom.push(CustomItem {
            id: id.clone(),
            text: text.to_string(),
            due: due.to_string(),
            done: false,
            prio: prio.to_string(),
            note: note.to_string(),
            img: img.to_string(),
            extra: BTreeMap::new(),
        });
        Some(id)
    }

    pub fn done_count(&self) -> usize {
        self.done.values().filter(|v| **v).count()
    }

    pub fn summary(&self) -> String {
        format!(
            "custom={} department={} archive={} done={} tab={} win_mode={}",
            self.custom.len(),
            self.department.len(),
            self.archive.len(),
            self.done_count(),
            self.ui.tab,
            self.ui.win_mode
        )
    }

    pub fn archive_list(&self) -> &[ArchiveItem] {
        &self.archive
    }

    /// 软删除 / 完成归档：从 department 或 custom 移除并推入 archive。
    /// 同一 id 只保留一条，重复归档时覆盖 archivedAt 与 reason 为最新。
    /// 返回是否命中待办项。
    /// 启动/重载时把「已完成」的待办直接沉归档（勾上即走，列表里不再留删除线）
    pub fn sweep_done(&mut self) -> usize {
        let ids: Vec<String> = self
            .department
            .iter()
            .filter(|d| d.done)
            .map(|d| d.id.clone())
            .chain(self.custom.iter().filter(|c| c.done).map(|c| c.id.clone()))
            .collect();
        let mut n = 0usize;
        for id in ids {
            if self.archive_remove(&id, "done") {
                n += 1;
            }
        }
        n
    }

    pub fn archive_remove(&mut self, id: &str, reason: &str) -> bool {
        let reason = if ArchiveItem::is_valid_reason(reason) {
            reason
        } else {
            "deleted"
        };

        #[allow(clippy::type_complexity)]
        let taken: Option<(
            String,
            String,
            Option<String>,
            bool,
            String,
            String,
            String,
            String,
        )> = if let Some(pos) = self.department.iter().position(|d| d.id == id) {
            let d = self.department.remove(pos);
            Some((
                d.id,
                d.text,
                d.due,
                d.done,
                "yynote".to_string(),
                d.prio,
                d.note,
                String::new(),
            ))
        } else if let Some(pos) = self.custom.iter().position(|c| c.id == id) {
            let c = self.custom.remove(pos);
            let due = if c.due.is_empty() { None } else { Some(c.due) };
            Some((
                c.id,
                c.text,
                due,
                c.done,
                "user".to_string(),
                c.prio,
                c.note,
                c.img,
            ))
        } else {
            None
        };

        let (id, text, due, done, src, prio, note, img) = match taken {
            Some(v) => v,
            None => return false,
        };

        // 同 id 不重复：先清掉旧条目，再以最新 archivedAt 入档
        self.archive.retain(|a| a.id != id);
        self.archive.push(ArchiveItem {
            id,
            text,
            due,
            done,
            src,
            prio,
            note,
            img,
            archived_at: crate::log::now_iso_local(),
            reason: reason.to_string(),
            extra: BTreeMap::new(),
        });
        true
    }

    /// 恢复：按 src 放回 department / custom，done 保持归档时的值。
    pub fn archive_restore(&mut self, id: &str) -> bool {
        let pos = match self.archive.iter().position(|a| a.id == id) {
            Some(p) => p,
            None => return false,
        };
        let a = self.archive.remove(pos);
        // 恢复出来的待办必须是「未完成」，否则启动时的 sweep_done 会把它又归档回去。
        self.done.retain(|k, _| !k.contains(&a.id));
        if a.src == "yynote" {
            self.department.retain(|d| d.id != id);
            self.department.push(DepartmentItem {
                id: a.id,
                text: a.text,
                due: a.due,
                done: false,
                src: "yynote".to_string(),
                imported_at: a.archived_at,
                prio: a.prio,
                note: a.note,
                extra: a.extra,
            });
        } else {
            self.custom.retain(|c| c.id != id);
            self.custom.push(CustomItem {
                id: a.id,
                text: a.text,
                due: a.due.unwrap_or_default(),
                done: false,
                prio: a.prio,
                note: a.note,
                img: a.img,
                extra: a.extra,
            });
        }
        true
    }

    /// 彻底删除：只从 archive 移除，其它段不动。
    pub fn archive_purge(&mut self, id: &str) -> bool {
        let before = self.archive.len();
        self.archive.retain(|a| a.id != id);
        self.archive.len() != before
    }

    /// 清掉重复 id（保留最后一条）与非法 reason，用于加载后自愈。
    fn normalize_archive(&mut self) -> bool {
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for (i, a) in self.archive.iter().enumerate() {
            seen.insert(a.id.clone(), i);
        }
        let mut changed = false;
        if seen.len() != self.archive.len() {
            let keep: Vec<usize> = seen.values().copied().collect();
            let mut out = Vec::with_capacity(keep.len());
            for (i, item) in self.archive.drain(..).enumerate() {
                if keep.contains(&i) {
                    out.push(item);
                }
            }
            self.archive = out;
            changed = true;
        }
        for a in self.archive.iter_mut() {
            if !ArchiveItem::is_valid_reason(&a.reason) {
                a.reason = "deleted".to_string();
                changed = true;
            }
        }
        changed
    }
}

/// 20261110142530，用于 .bad 备份名与 custom id。
pub fn compact_ts() -> String {
    now_str() // "YYYY-MM-DD HH:MM:SS"
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect()
}

/// 错误日志固定放状态文件同目录：research/widget_state_error.log
pub fn error_log_path(state: &Path) -> PathBuf {
    state
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("widget_state_error.log")
}

/// 读状态。缺失 → 新结构；损坏 → 备份 .bad-<ts> 后重建；老格式 → 补齐 department/ui 并落盘。
pub fn load(path: &Path, yynote: &Path, log: &Logger) -> AppState {
    let errlog = Logger::new(error_log_path(path));
    let mut need_save = false;

    let mut state = match std::fs::read_to_string(path) {
        Err(e) => {
            log.line(&format!(
                "[state] read failed path={} err={} -> new empty state",
                path.display(),
                e
            ));
            need_save = true;
            AppState::default()
        }
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(v) => {
                let has_dept = v.get("department").is_some();
                let has_ui = v.get("ui").is_some();
                let has_archive = v.get("archive").is_some();
                match serde_json::from_value::<AppState>(v) {
                    Ok(mut s) => {
                        log.line(&format!(
                            "[state] loaded {} bytes {} old_format={}",
                            text.len(),
                            s.summary(),
                            !(has_dept && has_ui && has_archive)
                        ));
                        if !has_dept || !has_ui || !has_archive {
                            log.line(&format!(
                                "[state] migrate missing department={} ui={} archive={} -> filled with defaults",
                                !has_dept, !has_ui, !has_archive
                            ));
                            need_save = true;
                        }
                        if s.normalize_archive() {
                            log.line("[state] archive normalized (dedup id / invalid reason fixed)");
                            need_save = true;
                        }
                        s
                    }
                    Err(e) => {
                        errlog.line(&format!(
                            "[state] shape mismatch path={} err={} -> rebuilt empty",
                            path.display(),
                            e
                        ));
                        backup_bad(path, &errlog);
                        need_save = true;
                        AppState::default()
                    }
                }
            }
            Err(e) => {
                errlog.line(&format!(
                    "[state] parse failed path={} err={} -> backup + rebuilt empty",
                    path.display(),
                    e
                ));
                backup_bad(path, &errlog);
                need_save = true;
                AppState::default()
            }
        },
    };

    // department 导入 / 合并（本地 done 优先）
    if merge_yynote(&mut state, yynote, &errlog, log) {
        need_save = true;
    }

    if need_save
        && !save(path, &state, log) {
            errlog.line(&format!("[state] save after load failed path={}", path.display()));
        }
    state
}

fn backup_bad(path: &Path, errlog: &Logger) {
    let bad = PathBuf::from(format!("{}.bad-{}", path.display(), compact_ts()));
    match std::fs::rename(path, &bad) {
        Ok(()) => errlog.line(&format!("[state] backup ok -> {}", bad.display())),
        Err(e) => errlog.line(&format!(
            "[state] backup failed src={} err={}",
            path.display(),
            e
        )),
    }
}

/// 从 yynote_todos.json 导入：缺文件不报错；已有同 id 项保留本地 done。
/// 返回是否改动了状态（需要落盘）。
fn merge_yynote(state: &mut AppState, yynote: &Path, errlog: &Logger, log: &Logger) -> bool {
    let text = match std::fs::read_to_string(yynote) {
        Ok(t) => t,
        Err(_) => {
            log.line(&format!(
                "[state] yynote not available path={} -> department kept ({} items)",
                yynote.display(),
                state.department.len()
            ));
            return false;
        }
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            errlog.line(&format!(
                "[state] yynote parse failed path={} err={} -> department kept",
                yynote.display(),
                e
            ));
            return false;
        }
    };
    let arr = if let Some(a) = value.as_array() {
        a.clone()
    } else if let Some(a) = value.get("todos").and_then(|v| v.as_array()) {
        a.clone()
    } else if let Some(a) = value.get("items").and_then(|v| v.as_array()) {
        a.clone()
    } else {
        log.line("[state] yynote shape not recognized -> department kept");
        return false;
    };

    let now = now_str().replace(' ', "T");
    let mut added = 0usize;
    let mut kept = 0usize;
    let mut changed = false;
    // 文本归一化：按空白切分再用单空格 join，能压掉任意连续空白（含 tab/换行/多空格），
    // 比旧的 replace("  ", " ") 只替双空格更彻底，避免导出文本里的空白差异造成漏判重。
    let norm = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    // id 归一化：sync_yynote_todos.mjs 写的是 PC_Todo_…，本程序导入时加前缀 y，
    // 同一件事会变成 PC_Todo_x 与 yPC_Todo_x 两条 —— 比较时只剥「一个」前导 y，
    // 不能用 trim_start_matches('y')（会把开头所有 y 吃掉、把不同 id 误判成同一条）。
    // 原 same_key / same_id 两个闭包逻辑完全相同，合并为一个。
    let same_key = |a: &str, b: &str| {
        let sa = a.strip_prefix('y').unwrap_or(a);
        let sb = b.strip_prefix('y').unwrap_or(b);
        a == b || sa == sb
    };
    for item in arr {
        let raw_id = item
            .get("id")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        if raw_id.is_empty() {
            continue;
        }
        let id = format!("y{}", raw_id);
        let text = item
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if text.is_empty() {
            continue;
        }
        let due = match item.get("due") {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let done = item.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
        let nt = norm(&text);

        // 文本判重覆盖 department + custom + archive：小黄条导出每次重建 id，
        // 只按 id 判会把同一条待办反复加回部门列表（用户看到的重影）；自建项、归档项里
        // 已有同文本时也不能再导回，否则与用户自建重复、或「加一条又扫走」。
        let dup_dept = state
            .department
            .iter()
            .position(|d| d.id != id && norm(&d.text) == nt);
        if let Some(pos) = dup_dept {
            let existing = &mut state.department[pos];
            // 本地为准：done 不被旧 JSON 覆盖；只补空 due、同步文本变化，真改了才落盘
            if existing.due.is_none() && due.is_some() {
                existing.due = due;
                changed = true;
            }
            if existing.text != text {
                existing.text = text;
                changed = true;
            }
            kept += 1;
            continue;
        }
        if state.custom.iter().any(|c| norm(&c.text) == nt)
            || state.archive.iter().any(|a| norm(&a.text) == nt)
        {
            // 自建/归档里已有同文本：只挡重复，不改动用户自己的条目
            kept += 1;
            continue;
        }
        // 已在归档里的（无论 done/deleted，按 id）不要再导回部门列表，
        // 避免每次启动「加一条又扫走」。
        if state.archive.iter().any(|a| same_key(&a.id, &id)) {
            kept += 1;
            continue;
        }
        if let Some(existing) = state.department.iter_mut().find(|d| same_key(&d.id, &id)) {
            // 本地为准：done 不被旧 JSON 覆盖；文本变化才更新
            if existing.text != text {
                existing.text = text;
                changed = true;
            }
            if existing.due.is_none() && due.is_some() {
                existing.due = due;
                changed = true;
            }
            kept += 1;
        } else {
            state.department.push(DepartmentItem {
                id,
                text,
                due,
                done,
                src: "yynote".to_string(),
                imported_at: now.clone(),
                prio: String::new(),
                note: String::new(),
                extra: BTreeMap::new(),
            });
            added += 1;
            changed = true;
        }
    }
    if added > 0 || kept > 0 {
        log.line(&format!(
            "[state] yynote merged added={} kept_local={} changed={} department_total={}",
            added,
            kept,
            changed,
            state.department.len()
        ));
    }
    // 有实际变更才 true：kept 分支里 due/text 同步也算变更，需要落盘；
    // 旧写法只认 added > 0，会漏掉这些字段更新的持久化。
    changed
}

/// 写状态：<path>.tmp 落盘 + fsync + rename 覆盖；rename 失败（Windows 目标已存在）回退直接写目标。
/// 回退路径不保证原子性，日志以 atomic=rename / atomic=write_sync-fallback 区分，不虚报。
pub fn save(path: &Path, state: &AppState, log: &Logger) -> bool {
    let bytes = match serde_json::to_vec_pretty(state) {
        Ok(b) => b,
        Err(e) => {
            Logger::new(error_log_path(path)).line(&format!("[state] serialize failed err={}", e));
            return false;
        }
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp: PathBuf = path.with_extension("json.tmp");
    if !write_sync(&tmp, &bytes) {
        let _ = std::fs::remove_file(&tmp);
        log.line(&format!(
            "[state] write temp failed path={} bytes={}",
            tmp.display(),
            bytes.len()
        ));
        return false;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => {
            log.line(&format!(
                "[state] saved path={} bytes={} {} atomic=rename",
                path.display(),
                bytes.len(),
                state.summary()
            ));
            true
        }
        Err(e) => {
            // Windows 上 rename 不能覆盖已存在的目标（ERROR_ALREADY_EXISTS）。先删目标再 rename
            // 会留一个「原文件已删、新文件没落」的空窗，崩溃即丢状态，风险高；改为回退到直接写目标：
            // 保留原目标直到 write_sync 成功覆盖，日志如实标 atomic=write_sync-fallback，不虚报原子性。
            log.line(&format!(
                "[state] rename failed tmp={} dst={} err={} -> fallback write_sync",
                tmp.display(),
                path.display(),
                e
            ));
            let ok = write_sync(path, &bytes);
            let _ = std::fs::remove_file(&tmp);
            if ok {
                log.line(&format!(
                    "[state] saved path={} bytes={} {} atomic=write_sync-fallback",
                    path.display(),
                    bytes.len(),
                    state.summary()
                ));
            } else {
                log.line(&format!(
                    "[state] fallback write_sync failed path={}",
                    path.display()
                ));
            }
            ok
        }
    }
}

fn write_sync(path: &Path, bytes: &[u8]) -> bool {
    use std::io::Write;
    match std::fs::File::create(path) {
        Ok(mut f) => {
            if let Err(e) = f.write_all(bytes) {
                eprintln!("[state] write_all failed path={} err={}", path.display(), e);
                return false;
            }
            if let Err(e) = f.sync_all() {
                eprintln!("[state] sync_all failed path={} err={}", path.display(), e);
                return false;
            }
            true
        }
        Err(e) => {
            eprintln!("[state] create failed path={} err={}", path.display(), e);
            false
        }
    }
}

/// 自测专用：确保 department 至少有一条，返回其 id。
/// 沙箱里没有 yynote 数据时 department 为空，4b/4c/4d 用空 id 必然失败。
fn ensure_dept_seed(st: &mut AppState) -> String {
    if st.department.is_empty() {
        st.department.push(DepartmentItem {
            prio: String::new(),
            note: String::new(),
            id: "seed_dept_1".to_string(),
            text: "自测部门项".to_string(),
            due: Some("2026-11-12".to_string()),
            done: false,
            src: "yynote".to_string(),
            imported_at: String::new(),
            extra: Default::default(),
        });
    }
    st.department
        .first()
        .map(|d| d.id.clone())
        .unwrap_or_default()
}

/// `--selftest`：在 --state 路径上验证 原子写 / 无 BOM / 往返一致 / 老格式迁移。
pub fn selftest(path: &Path, yynote: &Path, log: &Logger) -> bool {
    // 1) 老格式迁移：手写 {"custom":[],"done":{}} → 补齐 department/ui，custom/done 不变
    let legacy = bridge_legacy(path);
    log.line(&format!("[selftest] legacy wrote path={}", path.display()));
    let migrated = load(path, yynote, log);
    let mut ok = migrated.custom.is_empty() && migrated.done.is_empty();
    let file_after = std::fs::read_to_string(path).unwrap_or_default();
    let has_dept = file_after.contains("\"department\"");
    let has_ui = file_after.contains("\"ui\"");
    log.line(&format!(
        "[selftest] legacy migrated custom={} done={} department_key={} ui_key={} legacy_bytes={}",
        migrated.custom.len(),
        migrated.done.len(),
        has_dept,
        has_ui,
        legacy
    ));
    ok &= has_dept && has_ui;

    // 2) 原子写 / 无 BOM / 往返
    let mut st = migrated;
    st.set_done("ITEM_X|工装T0|2026-11-11", true);
    st.add_custom("自测条目 · UTF-8 中文", "2026-11-12");
    let s1 = save(path, &st, log);
    log.line(&format!("[selftest] save1_ok={}", s1));
    ok &= s1;

    let raw = std::fs::read(path).unwrap_or_default();
    let bom = raw.starts_with(&[0xEF, 0xBB, 0xBF]);
    let tmp_left = path.with_extension("json.tmp").exists();
    let utf8 = std::str::from_utf8(&raw).is_ok();
    log.line(&format!(
        "[selftest] file bytes={} bom={} utf8={} tmp_left={}",
        raw.len(),
        bom,
        utf8,
        tmp_left
    ));
    ok &= !bom && !tmp_left && utf8;

    let back = load(path, yynote, log);
    let roundtrip = back.done_count() == 1
        && back.custom.len() == 1
        && back.custom[0].text == "自测条目 · UTF-8 中文"
        && back.is_done("ITEM_X|工装T0|2026-11-11");
    log.line(&format!(
        "[selftest] roundtrip_ok={} {} first_custom={}",
        roundtrip,
        back.summary(),
        back.custom.first().map(|c| c.text.as_str()).unwrap_or("")
    ));
    ok &= roundtrip;

    // 3) 取消勾选不残留 key
    let mut st2 = back.clone();
    st2.set_done("ITEM_X|工装T0|2026-11-11", false);
    let s2 = save(path, &st2, log);
    let undone = !st2.is_done("ITEM_X|工装T0|2026-11-11");
    log.line(&format!("[selftest] save2_ok={} undone={}", s2, undone));
    ok &= s2 && undone;

    // 4) 归档：软删除 / 恢复 / 彻底删除 / 20 次循环不膨胀
    let has_archive_key = std::fs::read_to_string(path)
        .unwrap_or_default()
        .contains("\"archive\"");
    log.line(&format!("[selftest] archive_key_present={}", has_archive_key));
    ok &= has_archive_key;

    let mut st3 = back.clone();
    let cid = st3.custom.first().map(|c| c.id.clone()).unwrap_or_default();
    let removed = st3.archive_remove(&cid, "deleted");
    let in_archive = st3.archive_list().iter().any(|a| a.id == cid);
    let gone = !st3.custom.iter().any(|c| c.id == cid);
    let first = st3.archive_list().iter().find(|a| a.id == cid).cloned();
    log.line(&format!(
        "[selftest] archive_remove id={} ok={} src_gone={} in_archive={} reason={} archived_at={}",
        cid,
        removed,
        gone,
        in_archive,
        first.as_ref().map(|a| a.reason.as_str()).unwrap_or(""),
        first.as_ref().map(|a| a.archived_at.as_str()).unwrap_or("")
    ));
    ok &= removed && in_archive && gone;

    // 4a) 落盘后重新加载，归档仍在（模拟重启）
    ok &= save(path, &st3, log);
    let mut st4 = load(path, yynote, log);
    let persisted = st4.archive_list().iter().any(|a| a.id == cid);
    log.line(&format!(
        "[selftest] archive_persisted={} archive_len={}",
        persisted,
        st4.archive.len()
    ));
    ok &= persisted;

    // 4b) 完成归档 + 恢复：done 保持、回原数组、archive 移除
    let dept_id = ensure_dept_seed(&mut st4);
    if let Some(d) = st4.department.iter_mut().find(|d| d.id == dept_id) {
        d.done = true;
    }
    let moved = st4.archive_remove(&dept_id, "done");
    let done_in_archive = st4
        .archive_list()
        .iter()
        .find(|a| a.id == dept_id)
        .map(|a| a.done)
        .unwrap_or(false);
    let restored = st4.archive_restore(&dept_id);
    // 设计语义：恢复回列表时 done 置 false，否则启动 sweep_done 会立刻把它再次扫回归档。
    let back_in_dept = st4.department.iter().any(|d| d.id == dept_id && !d.done);
    let left_archive = !st4.archive_list().iter().any(|a| a.id == dept_id);
    log.line(&format!(
        "[selftest] archive_restore id={} moved={} done_in_archive={} restored={} back_in_dept_undone={} archived_left={}",
        dept_id, moved, done_in_archive, restored, back_in_dept, left_archive
    ));
    ok &= moved && done_in_archive && restored && back_in_dept && left_archive;

    // 4c) 彻底删除：只从 archive 移除
    let _ = st4.archive_remove(&dept_id, "deleted");
    let dept_before = st4.department.len();
    let custom_before = st4.custom.len();
    let purged = st4.archive_purge(&dept_id);
    let purged_prev = st4.archive_purge(&cid);
    let purge_clean = !st4.archive_list().iter().any(|a| a.id == dept_id || a.id == cid)
        && st4.department.len() == dept_before
        && st4.custom.len() == custom_before
        && st4.archive.is_empty();
    log.line(&format!(
        "[selftest] archive_purge id={} ok={} prev_ok={} other_sections_intact={} archive_len={}",
        dept_id,
        purged,
        purged_prev,
        purge_clean,
        st4.archive.len()
    ));
    ok &= purged && purge_clean;

    // 4d) 20 次归档-恢复循环：文件大小稳定、无重复 id
    let mut sizes: Vec<usize> = Vec::new();
    let mut cycle_ok = true;
    let mut st5 = st4;
    let cycle_id = ensure_dept_seed(&mut st5);
    for _ in 0..20 {
        st5.archive_remove(&cycle_id, "deleted");
        st5.archive_restore(&cycle_id);
        // 同一 id 连续归档两次：只允许留一条，archivedAt 取最新
        st5.archive_remove(&cycle_id, "deleted");
        st5.archive_remove(&cycle_id, "done");
        let mut ids: Vec<&str> = st5.archive_list().iter().map(|a| a.id.as_str()).collect();
        ids.sort_unstable();
        let dedup = ids.windows(2).all(|w| w[0] != w[1]);
        let archived = st5.archive.len();
        st5.archive_restore(&cycle_id);
        if !dedup || archived != 1 || !st5.department.iter().any(|d| d.id == cycle_id) {
            if cycle_ok {
                log.line(&format!(
                    "[selftest] cycle_fail dedup={} archived={} in_dept={} cycle_id={}",
                    dedup,
                    archived,
                    st5.department.iter().any(|d| d.id == cycle_id),
                    cycle_id
                ));
            }
            cycle_ok = false;
        }
        if !save(path, &st5, log) {
            if cycle_ok {
                log.line("[selftest] cycle_fail save=false");
            }
            cycle_ok = false;
        }
        let bytes = std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
        sizes.push(bytes);
    }
    let stable = sizes.iter().all(|s| *s == sizes[0]);
    log.line(&format!(
        "[selftest] archive_cycle20 rounds={} stable={} bytes_first={} bytes_last={} archive_len={}",
        sizes.len(),
        stable,
        sizes.first().copied().unwrap_or(0),
        sizes.last().copied().unwrap_or(0),
        st5.archive.len()
    ));
    ok &= cycle_ok && stable && st5.archive.is_empty();

    log.line(&format!(
        "[selftest] result={}",
        if ok { "PASS" } else { "FAIL" }
    ));
    ok
}

/// 覆盖写一个最小老格式文件，返回字节数。
fn bridge_legacy(path: &Path) -> usize {
    let body = "{\"custom\":[],\"done\":{}}";
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(path, body.as_bytes());
    body.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        // 用 target/test-tmp（工作区内）而不是系统临时目录：本机沙箱对 %TEMP% 之外的写有路径限制。
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("{}-{}", tag, compact_ts()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("widget_state.json")
    }

    fn write(path: &Path, body: &str) {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        std::fs::write(path, body.as_bytes()).unwrap();
    }

    fn logger(path: &Path) -> Logger {
        Logger::new(path.with_extension("test.log"))
    }

    /// 旧文件无 archive：启动后补空数组，custom/department/done/ui 全部不变。
    #[test]
    fn old_file_gets_empty_archive_without_losing_other_sections() {
        let p = sandbox("old");
        write(
            &p,
            r#"{"custom":[{"id":"c1","text":"自建","due":"2026-11-12","done":false}],
                "department":[{"id":"y1","text":"部门项","due":null,"done":true,"src":"yynote","importedAt":"2026-11-01T09:00:00"}],
                "done":{"ITEM_X|工装T0|2026-11-11":true},
                "ui":{"tab":"dept","win_mode":"float","x":100,"y":200,"opacity":0.5}}"#,
        );
        let lg = logger(&p);
        let st = load(&p, &p.with_file_name("nope.json"), &lg);

        assert!(st.archive.is_empty(), "archive 应为空数组");
        assert_eq!(st.custom.len(), 1);
        assert_eq!(st.custom[0].text, "自建");
        assert_eq!(st.department.len(), 1);
        assert!(st.department[0].done);
        assert!(st.is_done("ITEM_X|工装T0|2026-11-11"));
        assert_eq!(st.ui.tab, "dept");
        assert_eq!(st.ui.win_mode, "float");
        assert_eq!(st.ui.x, Some(100));
        assert_eq!(st.ui.opacity, 0.5);

        let disk = std::fs::read_to_string(&p).unwrap();
        assert!(disk.contains("\"archive\""), "落盘后应含 archive 段");
        let back: Value = serde_json::from_str(&disk).unwrap();
        assert_eq!(back["archive"].as_array().map(|a| a.len()), Some(0));
        assert_eq!(back["custom"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(back["ui"]["win_mode"], "float");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 同一 id 在 archive 里重复：加载后只留一条，archivedAt 取最新。
    #[test]
    fn duplicate_archive_ids_collapse_to_latest() {
        let p = sandbox("dup");
        write(
            &p,
            r#"{"custom":[],"department":[],"done":{},"ui":{},"archive":[
                {"id":"y9","text":"旧","due":null,"done":false,"src":"yynote","archivedAt":"2026-11-01T10:00:00+08:00","reason":"deleted"},
                {"id":"y9","text":"新","due":"2026-11-05","done":true,"src":"yynote","archivedAt":"2026-11-09T18:30:00+08:00","reason":"done"}]}"#,
        );
        let lg = logger(&p);
        let st = load(&p, &p.with_file_name("nope.json"), &lg);
        assert_eq!(st.archive.len(), 1, "同 id 只能留一条");
        assert_eq!(st.archive[0].archived_at, "2026-11-09T18:30:00+08:00");
        assert_eq!(st.archive[0].reason, "done");
        assert_eq!(st.archive[0].text, "新");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 归档 item 字段齐全、reason 只允许 done|deleted、恢复后 done 不变、purge 只动 archive。
    #[test]
    fn archive_item_shape_and_reason_guard() {
        let p = sandbox("shape");
        let lg = logger(&p);
        let mut st = AppState::default();
        st.department.push(DepartmentItem {
            id: "yPC_Todo_1".into(),
            text: "部门项".into(),
            due: Some("2026-11-04".into()),
            done: true,
            prio: String::new(),
            note: String::new(),
            src: "yynote".into(),
            imported_at: "2026-11-01T09:00:00".into(),
            extra: BTreeMap::new(),
        });
        st.custom.push(CustomItem {
            id: "c1".into(),
            text: "自建项".into(),
            due: "2026-11-12".into(),
            done: false,
            prio: String::new(),
            note: String::new(),
            img: String::new(),
            extra: BTreeMap::new(),
        });

        // 非法 reason 兜底为 deleted
        assert!(st.archive_remove("c1", "whatever"));
        assert_eq!(st.archive[0].reason, "deleted");
        assert!(!st.custom.iter().any(|c| c.id == "c1"));

        // done 归档保留 done=true
        assert!(st.archive_remove("yPC_Todo_1", "done"));
        let a = st.archive_list().iter().find(|a| a.id == "yPC_Todo_1").unwrap();
        assert!(a.done);
        assert_eq!(a.due.as_deref(), Some("2026-11-04"));
        assert_eq!(a.src, "yynote");
        assert!(a.archived_at.contains('T') && a.archived_at.contains('+'), "归档时间应为 ISO+时区: {}", a.archived_at);

        // 落盘再读：字段原样
        assert!(save(&p, &st, &lg));
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let obj = &raw["archive"][1];
        for k in ["id", "text", "due", "done", "src", "archivedAt", "reason"] {
            assert!(obj.get(k).is_some(), "archive 项缺字段 {k}");
        }
        assert_eq!(obj["reason"], "done");

        let mut st2 = load(&p, &p.with_file_name("nope.json"), &lg);
        assert!(st2.archive_restore("yPC_Todo_1"));
        // 恢复语义：回 department 且 done 置 false，否则启动 sweep_done 会立刻再归档（历史 bug）。
        assert!(st2
            .department
            .iter()
            .any(|d| d.id == "yPC_Todo_1" && !d.done));
        assert!(!st2.is_done("yPC_Todo_1"));
        assert!(!st2.archive_list().iter().any(|a| a.id == "yPC_Todo_1"));

        // purge 只动 archive
        let dept = st2.department.len();
        let cust = st2.custom.len();
        let done = st2.done.len();
        assert!(st2.archive_list().iter().any(|a| a.id == "c1"));
        let _ = st2.archive_purge("c1");
        assert!(st2.archive.is_empty());
        assert_eq!(st2.department.len(), dept);
        assert_eq!(st2.custom.len(), cust);
        assert_eq!(st2.done.len(), done);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 20 次归档-恢复循环后文件大小稳定、无重复 id。
    #[test]
    fn twenty_cycles_do_not_grow_the_file() {
        let p = sandbox("cycle");
        let lg = logger(&p);
        let mut st = AppState::default();
        for i in 0..3 {
            st.department.push(DepartmentItem {
                id: format!("yPC_Todo_{i}"),
                text: format!("部门项{i}"),
                due: Some("2026-11-05".into()),
                done: false,
                prio: String::new(),
                note: String::new(),
                src: "yynote".into(),
                imported_at: "2026-11-01T09:00:00".into(),
                extra: BTreeMap::new(),
            });
        }
        let mut sizes = Vec::new();
        for _ in 0..20 {
            st.archive_remove("yPC_Todo_0", "deleted");
            st.archive_restore("yPC_Todo_0");
            st.archive_remove("yPC_Todo_0", "deleted");
            st.archive_remove("yPC_Todo_0", "done"); // 同 id 二次归档
            assert_eq!(st.archive.len(), 1, "同 id 不应产生两条");
            st.archive_restore("yPC_Todo_0");
            assert!(save(&p, &st, &lg));
            sizes.push(std::fs::metadata(&p).unwrap().len());
        }
        assert!(sizes.iter().all(|s| *s == sizes[0]), "文件大小应稳定: {:?}", sizes);
        assert_eq!(st.archive.len(), 0);
        assert_eq!(st.department.len(), 3);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}

/// 拖拽排序用的「载荷」：两种待办的公共字段（due 统一成 Option）
#[derive(Clone, Default)]
pub struct TodoPayload {
    pub text: String,
    pub due: Option<String>,
    pub done: bool,
    pub prio: String,
    pub note: String,
    pub img: String,
}

impl AppState {
    fn todo_take(&mut self, grp: u8, idx: usize) -> Option<(TodoPayload, String)> {
        if grp == 1 {
            if idx >= self.department.len() {
                return None;
            }
            let it = self.department.remove(idx);
            Some((
                TodoPayload {
                    text: it.text,
                    due: it.due,
                    done: it.done,
                    prio: it.prio,
                    note: it.note,
                    img: String::new(),
                },
                it.id,
            ))
        } else {
            if idx >= self.custom.len() {
                return None;
            }
            let it = self.custom.remove(idx);
            Some((
                TodoPayload {
                    text: it.text,
                    due: if it.due.is_empty() { None } else { Some(it.due) },
                    done: it.done,
                    prio: it.prio,
                    note: it.note,
                    img: it.img,
                },
                it.id,
            ))
        }
    }

    fn todo_insert(&mut self, grp: u8, idx: usize, p: TodoPayload, id: String) {
        if grp == 1 {
            let at = idx.min(self.department.len());
            self.department.insert(
                at,
                DepartmentItem {
                    id,
                    text: p.text,
                    due: p.due,
                    done: p.done,
                    prio: p.prio,
                    note: p.note,
                    src: String::new(),
                    imported_at: String::new(),
                    extra: BTreeMap::new(),
                },
            );
        } else {
            let at = idx.min(self.custom.len());
            self.custom.insert(
                at,
                CustomItem {
                    id,
                    text: p.text,
                    due: p.due.unwrap_or_default(),
                    done: p.done,
                    prio: p.prio,
                    note: p.note,
                    img: p.img,
                    extra: BTreeMap::new(),
                },
            );
        }
    }

    /// 把第 from 组第 fi 项移动到第 to 组第 ti 项的位置（可跨组，跨组时转换结构）。
    pub fn move_todo_to(&mut self, fg: u8, fi: usize, tg: u8, ti: usize) -> bool {
        if fg == tg {
            if fg == 1 {
                if fi < self.department.len() && ti < self.department.len() && fi != ti {
                    let it = self.department.remove(fi);
                    self.department.insert(ti, it);
                    return true;
                }
            } else if fi < self.custom.len() && ti < self.custom.len() && fi != ti {
                let it = self.custom.remove(fi);
                self.custom.insert(ti, it);
                return true;
            }
            return false;
        }
        match self.todo_take(fg, fi) {
            Some((p, id)) => {
                self.todo_insert(tg, ti, p, id);
                true
            }
            None => false,
        }
    }
}

/// 备注各行的样式（加粗/标题/列表/编号）——与正文分开存，正文里不再出现 ** 或 "- " 这类标记。
fn notes_path(state: &std::path::Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}.notes.json", state.display()))
}

fn notes_map(state: &std::path::Path) -> serde_json::Map<String, serde_json::Value> {
    let p = notes_path(state);
    match std::fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str::<serde_json::Value>(&t)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default(),
        Err(_) => serde_json::Map::new(),
    }
}

pub fn load_note_style(state: &std::path::Path, id: &str) -> String {
    notes_map(state)
        .get(id)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn save_note_style(state: &std::path::Path, id: &str, style: &str) {
    let mut map = notes_map(state);
    if style.trim_matches(',').is_empty() {
        map.remove(id);
    } else {
        map.insert(id.to_string(), serde_json::Value::String(style.to_string()));
    }
    let p = notes_path(state);
    if let Ok(t) = serde_json::to_string(&serde_json::Value::Object(map)) {
        let _ = std::fs::write(p, t);
    }
}
