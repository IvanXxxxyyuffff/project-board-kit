// 命令行：--data / --state / --log / --x / --y / --reload-secs / --headless / --selftest / --seconds
// 默认值面向本机：data+state 在 <ROOT>\research，X=24，Y=工作区底部对齐。
//
// 分发形态（t5）：exe 放 <ROOT>\tools\项目看板.exe，
// 状态/日志走「exe 同目录可写 → %LOCALAPPDATA%\项目看板\」两级，
// 数据仍读管道产出的 research\widget_data.json（只读）。
use std::path::{Path, PathBuf};

pub const DEFAULT_DATA: &str = r"research\widget_data.json";
pub const DEFAULT_STATE: &str = r"research\widget_state.json";
pub const DEFAULT_YYNOTE: &str = r"research\yynote_todos.json";
pub const APP_VERSION: &str = "2.33.2";
pub const DEFAULT_X: i32 = 24;
pub const DEFAULT_RELOAD_SECS: u32 = 30;
/// 状态文件默认名（两级目录通用）
pub const STATE_FILE: &str = "widget_state.json";
pub const LOG_FILE: &str = "rust-widget.log";
pub const APP_DIR_NAME: &str = "项目看板";


/// 启动时登记的 --data 绝对位置（OnceLock，首次 set 生效）；resolve_rel 用它兜底定位相对文件。
static DATA_FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// 启动时登记 --data 的绝对位置；用于「看板 html / yynote 导出」等相对文件定位
pub fn set_data_file(p: &std::path::Path) {
    let _ = DATA_FILE.set(p.to_path_buf());
}

/// 相对默认路径 → 实际路径。候选顺序：① cwd；② exe 目录及其上 1~3 级；
/// ③ `set_data_file` 登记的 --data 父目录及其上 4 级。取首个 `is_file()` 命中；
/// 全部 miss 时返回首个候选（至少是合法的相对拼接），由调用方按「文件不存在」报错，而不是直接 panic。
/// 先信任安装目录（exe 及其祖先）再信任 --data：便携分发时数据固定跟 exe 走，
/// --data 只是用户临时指定的一份覆盖数据，其目录层级不该压过随程序发布的那份稳定数据。
pub fn resolve_rel(rel: &str) -> PathBuf {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        cands.push(cwd.join(rel));
    }
    if let Some(dir) = exe_dir() {
        cands.push(dir.join(rel));
        let mut up = dir.clone();
        for _ in 0..3 {
            if let Some(p) = up.parent().map(|x| x.to_path_buf()) {
                cands.push(p.join(rel));
                up = p;
            }
        }
    }
    if let Some(dp) = DATA_FILE.get() {
        let mut up = dp.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            let Some(d) = up else { break };
            cands.push(d.join(rel));
            up = d.parent().map(|p| p.to_path_buf());
        }
    }
    for c in cands.iter() {
        if c.is_file() {
            return c.clone();
        }
    }
    cands.into_iter().next().unwrap_or_else(|| PathBuf::from(rel))
}
/// exe 所在目录（便携放置时状态文件就落这里）
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// %LOCALAPPDATA%\项目看板 —— exe 目录不可写时的兜底存储
pub fn local_app_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join(APP_DIR_NAME))
}

/// 目录可写性探针：建一个隐藏临时文件再删掉（不污染目录）。
fn dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".rw-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 两级存储：exe 同目录（可写）→ %LOCALAPPDATA%\项目看板\
/// 都不可写时退回默认 research 路径（保持老行为，只读也先能跑）。
pub fn default_state_path() -> PathBuf {
    if let Some(dir) = exe_dir() {
        if dir_writable(&dir) {
            return dir.join(STATE_FILE);
        }
    }
    if let Some(dir) = local_app_dir() {
        if dir_writable(&dir) {
            return dir.join(STATE_FILE);
        }
    }
    PathBuf::from(DEFAULT_STATE)
}

/// 数据默认：管道产物 research\widget_data.json 存在就用它，
/// 否则找 exe 同目录的 widget_data.json（便携包/离线演示）。
pub fn default_data_path() -> PathBuf {
    let pipeline = resolve_rel(DEFAULT_DATA);
    if pipeline.is_file() {
        return pipeline;
    }
    if let Some(dir) = exe_dir() {
        let local = dir.join("widget_data.json");
        if local.is_file() {
            return local;
        }
    }
    pipeline
}

/// 小黄条待办来源：state 同目录 → research 官方路径，取第一个存在的
pub fn default_yynote_path(state: &Path) -> PathBuf {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(dir) = state.parent() {
        cands.push(dir.join("yynote_todos.json"));
    }
    cands.push(resolve_rel(DEFAULT_YYNOTE));
    cands
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .unwrap_or_else(|| resolve_rel(DEFAULT_YYNOTE))
}

#[derive(Debug, Clone)]
pub struct Config {
    pub data: PathBuf,
    pub state: PathBuf,
    pub yynote: PathBuf,
    pub log: Option<PathBuf>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub reload_secs: u32,
    pub headless: bool,
    pub selftest: bool,
    pub seconds: Option<u64>,
    /// 窗口层：embed（贴桌面层）/ normal（普通浮窗）/ topmost（置顶）；空 = 跟 state.ui.win_mode
    pub mode: String,
    /// 命中区导出路径（JSON，含 tab 维度）
    pub dump_hit: Option<PathBuf>,
    /// 字号基准（13 为默认；16 为上限），内部换算成百分比
    pub font_size: Option<i32>,
    /// 离线出图（无窗口时把当前帧写成 PNG，用于证据）
    pub snap: Option<PathBuf>,
    pub help: bool,
    pub opaque: bool,
    /// 离线演示态：confirm（删除二次确认）/ newtodo（行内新建）/ archive（归档视图）
    pub demo: String,
    /// 自启开关命令：on / off / toggle / status（执行后直接退出，不建窗口）
    pub autostart: Option<String>,
    /// 内置采集调度（t7）：默认开启，--no-pull 关掉
    pub pull: bool,
    /// 调度间隔覆盖（分钟，日夜同值；验收/调试用）
    pub pull_minutes: Option<u32>,
    /// 采集脚本，默认 <data 所在目录的上级>\每日项目简报.ps1
    pub pull_script: Option<PathBuf>,
    /// 跳过单实例互斥（--multi；验收要并行跑多个实例时用）
    pub multi: bool,
    /// 只打印调度计划后退出（--pull-plan）
    pub pull_plan: bool,
    /// --version：打印版本后退出（确认同事机器装的是哪一版）
    pub show_version: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            data: default_data_path(),
            state: default_state_path(),
            yynote: resolve_rel(DEFAULT_YYNOTE),
            log: None,
            x: None,
            y: None,
            reload_secs: DEFAULT_RELOAD_SECS,
        show_version: false,
            headless: false,
            selftest: false,
            seconds: None,
            mode: String::new(),
            dump_hit: None,
            font_size: None,
            snap: None,
            help: false,
            opaque: false,
            demo: String::new(),
            autostart: None,
            pull: true,
            pull_minutes: None,
            pull_script: None,
            multi: false,
            pull_plan: false,
        }
    }
}

impl Config {
    pub fn from_args() -> Config {
        // 启动时序：Config::default() 内 default_data_path()/resolve_rel() 就要定位相对文件，
        // 必须先把 --data 登记进 DATA_FILE，否则这条候选分支永远 miss。先取 argv 前置扫描一次，
        // 再 default()；主循环照常解析（含 --data 覆盖 c.data）。OnceLock 首次 set 生效，
        // main.rs 里后续 set_data_file(&cfg.data) 只是兜底（已 set 即 no-op）。
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut pre = 0;
        while pre < args.len() {
            let raw = &args[pre];
            if raw == "--data" {
                if let Some(v) = args.get(pre + 1) {
                    if !v.starts_with('-') {
                        set_data_file(Path::new(v));
                    }
                }
                break;
            } else if let Some(v) = raw.strip_prefix("--data=") {
                if !v.is_empty() {
                    set_data_file(Path::new(v));
                }
                break;
            }
            pre += 1;
        }
        let mut c = Config::default();
        let mut yynote_set = false;
        let mut i = 0;
        while i < args.len() {
            let raw = args[i].clone();
            let (flag, inline) = match raw.split_once('=') {
                Some((f, v)) => (f.to_string(), Some(v.to_string())),
                None => (raw.clone(), None),
            };
            let take = |i: &mut usize| -> Option<String> {
                if let Some(v) = inline.clone() {
                    return Some(v);
                }
                if *i + 1 < args.len() {
                    *i += 1;
                    return Some(args[*i].clone());
                }
                None
            };
            match flag.as_str() {
                "--data" => {
                    if let Some(v) = take(&mut i) {
                        c.data = PathBuf::from(v);
                    }
                }
                "--state" => {
                    if let Some(v) = take(&mut i) {
                        c.state = PathBuf::from(v);
                    }
                }
                "--yynote" => {
                    if let Some(v) = take(&mut i) {
                        c.yynote = PathBuf::from(v);
                        yynote_set = true;
                    }
                }
                "--log" => {
                    if let Some(v) = take(&mut i) {
                        c.log = Some(PathBuf::from(v));
                    }
                }
                "--x" => {
                    if let Some(v) = take(&mut i) {
                        c.x = v.parse().ok();
                    }
                }
                "--y" => {
                    if let Some(v) = take(&mut i) {
                        c.y = v.parse().ok();
                    }
                }
                "--reload-secs" => {
                    if let Some(v) = take(&mut i) {
                        c.reload_secs = v.parse().unwrap_or(DEFAULT_RELOAD_SECS).max(1);
                    }
                }
                "--seconds" => {
                    if let Some(v) = take(&mut i) {
                        c.seconds = v.parse().ok();
                    }
                }
                "--version" => {
                    c.show_version = true;
                }
                "--mode" => {
                    if let Some(v) = take(&mut i) {
                        c.mode = v;
                    }
                }
                "--dump-hit" => {
                    // 允许 --dump-hit 不带值（默认写到数据同级 hit_regions.json）；
                    // 带值时下一参数不能是另一个开关。
                    let explicit = match inline.clone() {
                        Some(v) => Some(v),
                        None => {
                            let next_ok = i + 1 < args.len() && !args[i + 1].starts_with('-');
                            if next_ok {
                                i += 1;
                                Some(args[i].clone())
                            } else {
                                None
                            }
                        }
                    };
                    c.dump_hit = Some(match explicit {
                        Some(v) => PathBuf::from(v),
                        None => {
                            let dir = c
                                .data
                                .parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| PathBuf::from("."));
                            dir.join("hit_regions.json")
                        }
                    });
                }
                "--opaque" => {
                c.opaque = true;
            }
            "--font-size" => {
                    if let Some(v) = take(&mut i) {
                        c.font_size = v.parse().ok();
                    }
                }
                "--snap" => {
                    if let Some(v) = take(&mut i) {
                        c.snap = Some(PathBuf::from(v));
                    }
                }
                "--autostart" => {
                    if let Some(v) = take(&mut i) {
                        c.autostart = Some(v);
                    }
                }
                "--demo" => {
                    if let Some(v) = take(&mut i) {
                        c.demo = v;
                    }
                }
                "--pull-minutes" => {
                    if let Some(v) = take(&mut i) {
                        c.pull_minutes = v.parse().ok();
                    }
                }
                "--pull-script" => {
                    if let Some(v) = take(&mut i) {
                        c.pull_script = Some(PathBuf::from(v));
                    }
                }
                "--no-pull" => c.pull = false,
                "--multi" => c.multi = true,
                "--pull-plan" => c.pull_plan = true,
                "--headless" => c.headless = true,
                "--selftest" => c.selftest = true,
                "-h" | "--help" => c.help = true,
                _ => {}
            }
            i += 1;
        }
        if c.x.is_none() {
            c.x = Some(DEFAULT_X);
        }
        // 未显式指定时按优先级挑一个存在的来源（state 同目录 → research 官方路径）
        if !yynote_set {
            c.yynote = default_yynote_path(&c.state);
            // 兜底：数据文件同目录的 yynote_todos.json（便携部署时 state 在 tools\，数据在 research\）
            if !c.yynote.is_file() {
                if let Some(dir) = c.data.parent() {
                    let cand = dir.join("yynote_todos.json");
                    if cand.is_file() {
                        c.yynote = cand;
                    }
                }
            }
        }
        c
    }

    /// 日志默认与状态文件同目录：tools\rust-widget.log
    pub fn log_path(&self) -> PathBuf {
        if let Some(p) = &self.log {
            return p.clone();
        }
        let dir = self
            .state
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        dir.join(LOG_FILE)
    }

    pub fn summary(&self) -> String {
        format!(
            "data={} state={} yynote={} log={} x={} y={} reload_secs={} headless={} selftest={} seconds={:?} pull={} pull_minutes={:?} multi={}",
            self.data.display(),
            self.state.display(),
            self.yynote.display(),
            self.log_path().display(),
            self.x.map(|v| v.to_string()).unwrap_or_else(|| "workarea-bottom".into()),
            self.y.map(|v| v.to_string()).unwrap_or_else(|| "workarea-bottom".into()),
            self.reload_secs,
            self.headless,
            self.selftest,
            self.seconds,
            self.pull,
            self.pull_minutes,
            self.multi
        )
    }
}

pub const USAGE: &str = "\
rust-widget --data <widget_data.json> --state <widget_state.json> [选项]
  --data <path>       数据文件，默认 <exe 附近的>research\\widget_data.json
  --state <path>      状态文件，默认 exe 同目录（不可写时 %LOCALAPPDATA%\\项目看板\\）widget_state.json
  --yynote <path>     小黄条待办来源，默认 state 同目录\\yynote_todos.json → research\\yynote_todos.json
  --log <path>        日志文件，默认 <state 所在目录>\\rust-widget.log
  --x <px>            窗口左边距，默认 24
  --y <px>            窗口上边距，默认工作区底部对齐
  --reload-secs <n>   mtime 轮询间隔，默认 30
  --mode <m>          窗口层：embed（贴桌面）/ normal / topmost，默认跟 state.ui.win_mode
  --dump-hit [path]   导出命中区 JSON（含 tab 维度），默认 <data 目录>\\hit_regions.json
  --font-size <px>    字号基准（默认 13，上限 16），版式栅格不变
  --snap <png>        不开窗时把当前帧写成 PNG（离线出图）
  --headless          不开窗，只跑数据加载与轮询（可配 --seconds）
  --seconds <n>       headless 运行时长（秒）
  --selftest          在 --state 路径上自测原子写/无 BOM/往返一致后退出
  --pull-minutes <n>  采集间隔覆盖（分钟，日夜同值；验收用）
  --pull-script <p>   采集脚本，默认 <data 上级>\\每日项目简报.ps1
  --no-pull           关掉内置采集调度（只热重载外部产出的数据）
  --pull-plan         打印日/夜间隔与 24 小时调度表后退出
  --multi             跳过单实例互斥（可并行起多个实例做验收）
";
