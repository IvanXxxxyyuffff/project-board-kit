# rust-widget — 「项目看板」桌面挂件（Rust 版）

用 Rust + Win32（`windows-sys`，无自绘框架、无 GPU 合成层）重写原 WPF 挂件。
当前状态：t3 完成：980×566 无边框半透明圆角窗口，双标签页（项目进度 / 部门进度）、
月历（农历 + 节气 + 节点点 + 翻月 + 下钻）、抓取待办 / 我的待办 / 归档三视图、勾选与增删持久化、
整数矩形命中链、三档窗口层（embed / normal / topmost）、托盘右键菜单。

## 本机工具链（全部在 D: 盘，C: 零残留）

| 项 | 路径 / 值 |
|---|---|
| RUSTUP_HOME | `<TOOLS>\rust\rustup`（User 环境变量） |
| CARGO_HOME | `<TOOLS>\rust\cargo`（User 环境变量） |
| cargo / rustc | `<TOOLS>\rust\cargo\bin`（也在 User PATH） |
| 版本 | rustc 1.99.1 / cargo 1.99.1，host `x86_64-pc-windows-gnu` |
| 链接器 | winlibs MinGW-w64 GCC 16.2.0（UCRT, posix-seh）`<TOOLS>\mingw64\bin\gcc.exe` |
| 代理 | `<TOOLS>\rust\cargo\config.toml` 里 `[http] proxy=http://127.0.0.1:<PORT>`，`check-revoke=false` |

无 MSVC，所以走 `x86_64-pc-windows-gnu`；链接器只在本 crate 的 `.cargo/config.toml` 里指定，不动系统 PATH，不需要管理员：

```toml
[target.x86_64-pc-windows-gnu]
linker = "<TOOLS>\\mingw64\\bin\\gcc.exe"
ar     = "<TOOLS>\\mingw64\\bin\\ar.exe"
```

## 构建 / 运行

```bash
cd rust-widget
cargo build --release                       # 产物 target/release/rust-widget.exe
./target/release/rust-widget.exe            # 双击同效
```

当前 exe 815,104 字节（v2.0.0 / rich-text build；历史 815,104 字节样本已废弃）

## 依赖（严格 <15）

`cargo tree` = 20 行（含根 crate），去重后 14 个 crate，其中 5 个是构建期 proc-macro（不进二进制）：

```
rust-widget
├── serde / serde_core / serde_derive        # 数据层反序列化
├── serde_json                                # 状态文件读写
├── itoa / memchr / zmij                      # serde_json 的传递依赖
└── windows-sys                               # 仅 Win32 子集 feature
     └── windows-targets → windows_x86_64_gnu
构建期：proc-macro2 / quote / syn / unicode-ident
```

不用 `windows` 官方高层 crate：它带 `windows-core`/`windows-implement`/`windows-interface`/`windows-result`/`windows-strings` + `syn`/`quote`/`proc-macro2`/`unicode-ident`，实测把依赖树顶到 18 个，直接破线。`windows-sys` 是同一套 Win32 API 的裸声明层，零 proc-macro。

## 窗口实现要点（t1）

- `WS_POPUP` + `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，启动时 `SetWindowPos(HWND_NOTOPMOST, …)` 明确不置顶；`TOOLWINDOW` 保证不进 Alt-Tab、不出现在任务栏。
- 圆角与半透明：CPU 侧把 980×566 的 premultiplied BGRA 缓冲（底色 `#12141A`、alpha 235、圆角半径 18px、边缘覆盖率抗锯齿）画好后，`CreateDIBSection` + `UpdateLayeredWindow(ULW_ALPHA)` 一次提交。无模糊、无 DWM 模糊层、无 GPU 依赖，缩放到任何 DPI 都是清晰的。
拖动不走 HTCAPTION（现役实现）：WM_NCHITTEST 固定返回 HTCLIENT；下方 :239 rebuild-check.txt 的 HTCAPTION 记录为历史复检，不代表现役代码。
- `WM_ERASEBKGND` 返回 1 抑制闪烁；`WM_PAINT` 只为满足 DWM 的绘制契约。
- DPI：`SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`。
- 每 2 秒 `GetProcessMemoryInfo` 采样，追加写到 exe 同目录 `mem-baseline.log`（`working_set` / `private_bytes` / `ws_peak`）。

## 渲染管线（t3）

三块，全部 CPU、无 GPU、无模糊：

1. `canvas.rs`：预乘 ARGB `Canvas{w,h,px:Vec<u32>}`，`blend_at` 是标准 premultiplied over
 `out = src*sa + dst*(1-sa)`、`oa = sa + da*(1-sa)`。字形 / 圆角 / 描边都走这一条路径，
 缓冲可直接喂 `UpdateLayeredWindow(ULW_ALPHA)`，不再按 alpha 二次加权。
2. `text.rs`：GDI `CreateFontW` + `TextOutW`，取白字黑底 DIB 的 `max(r,g,b)` 当覆盖率上色。
 位图缓存 key = `(文本, 字号, 粗细, 颜色, alpha)`，上限 3000 条，超限整表清空；
 数据 mtime 变化 / 翻月 / 字体比例变化才 `clear_cache()`，不每帧重画字形。
3. 静止层 blit：`layout::render_chrome()` 只画面板圆角、头部、两张卡片外框（不随内容变），存进 `Ctx.base`；
 每次重绘先 `copy_rect_from` 再叠「标签栏 + 左卡内容 + 右卡内容」。`Draw::Left/Right` 只 blit 对应矩形，
 `Draw::Full` 才整屏 blit。翻月 / 切标签都不做半透明二次混合，像素级可复现。

两个踩过的真 bug（已修，别回退）：

- 预乘 over 写错（`oa = da + sa*(1-sa)`）会让面板 alpha 掉到 18，整屏发白。正确式见上。
- GDI 字体链：`CreateFontW` 对不存在的字面名也返回非 NULL，且「Microsoft YaHei UI」是纯拉丁子集，
 中文会按字体链接落 SimSun（44px 粗体雅黑、正文宋体的怪象）。现在用
 `GetGlyphIndicesW(…, GGI_MARK_NONEXISTING_GLYPHS)` 查 `中` 是否 `0xFFFF` 选字面名，
 顺序 `Microsoft YaHei` `Microsoft YaHei UI` `SimSun`；`DBG_FACE=1` 启动打印实际命中字面。

## 交互与命中区（t3）

- 命中区是整数矩形数组，按 z 序 `iter().rev().find()` 功能异常返回，不逐像素判 alpha。
 `--dump-hit` 写 `{"tab":"proj|dept|archive","w":980,"h":566,"regions":[{id,x,y,w,h,kind,cursor,drag}]}`
 三份 JSON + combined，verifier 断言坐标，不靠截图标定。
- `Kind::draggable()` 只有 `DragBlank`（面板空白）与 `DragCalNonDay`（月历非日期格）；
 日期格、勾选圈、文字、按钮、标签一律不拖。光标：可拖区 `IDC_SIZEALL`，其余（含锁定态）`IDC_ARROW`。
- 命中顺序：标签按钮（`tab.proj`/`tab.dept`）最高 面板 / 卡片空白 月历格 / 翻月 勾选圈 文字 行。
- 列表是固定行数栅格（项目 7 行、部门 9 行、归档 9 行），行高与外框不随内容变，命中坐标稳定。
- 归档：页脚 `已归档 N ›` 进归档视图，每条 `恢复` / `×`（彻底删除，二次确认变红「确定?」，3 秒有效）；
 部门行的 `×` 是「移入归档」，不是物理删除。

## 命令行

```bash
[--mode embed|normal|topmost|float]   # 默认 embed（贴桌面层）；交互验收用 normal；浮窗用 float
                [--mode embed|normal|topmost]   # 默认 embed（贴桌面层）；交互验收用 normal
                [--dump-hit [P]]                # 导出命中区 JSON（默认 <data 目录>\hit_regions.json）
                [--snap P]                      # 配合 --dump-hit 同时出三张 PNG
                [--font-size N]                 # 基准字号，13 = 100%，clamp 80–140%
                [--opaque]                      # 面板底 alpha 顶到 255（验收截图用，去掉桌面透出，像素哈希才可比）
                [--demo confirm|newtodo|newtodo-date]
                                                # 离线证据态：只在 dump 里改内存（确认确认态/输入行/日期菜单），不写 state
                [--reload-secs N] [--headless] [--seconds N] [--selftest]
                [--pull-minutes N]              # 覆盖采集间隔（默认读 widget_config.json 的 pullMinutes，再缺省 60）
- 部署配置 `widget_config.json` 用 30/60（白天 30 分钟、夜间 60 分钟）；代码缺省 60/240；优先级 CLI --pull-minutes > dayMinutes/nightMinutes > pullMinutes > 默认 60/240。
                [--pull-script P]               # 采集脚本，默认 <data 上级目录>\每日项目简报.ps1
                [--no-pull]                     # 关掉内置调度（只热重载数据）
                [--pull-plan]                   # 打印调度表（哪个小时 night、间隔多少、是否撞 09:10）后退出
                [--multi]                       # 跳过单实例互斥（验收并发跑多个实例用）
                [--autostart on|off|toggle|status]  # 开机自启 lnk 增删查
```

环境变量（仅排障）：

| 变量 | 作用 |
|---|---|
| `RW_FLIP_TEST=1` + `--headless` | 进程内画布级自检：按 `Draw::Right` 真实路径做 nextprev 往返，打印 `SELFTEST_FLIP PASS/FAIL start/after1/end` |
| `RW_SCREEN_TEST=1` | 进程内窗口层自检：`BitBlt` 真实屏幕像素做单次往返与 20 次交替翻月，打印 `SCREEN_TEST` / `SCREEN_TEST20` |
| `RUST_WIDGET_TRACE=1` | `redraw` 打印 `[redraw] full|left|right xx.xxms`；`Draw::Right` 另打 `[blit] base_avg/canvas_before/canvas_after` |
| `DBG_FACE=1` | 打印 GDI 实际命中的字体（确认没走字体链接掉到 SimSun） |

### 证据图怎么出

```bash
./rust-widget.exe --headless --seconds 1 --demo confirm \
    --data ../research/widget_data.json --state ../research2/widget_state.json \
    --dump-hit out/hit_v5.json --snap out/v5.png
# → out/v5.{proj,dept,archive}.png + out/hit_v5{,_proj,_dept,_archive}.json
```

## 验收结果（2026-11-10，release 单二进制，`evidence/check_widget.ps1`）

| 模式 | 用例 | 结果 |
|---|---|---|
| `-Mode startup` | A01 启动可见 | PASS 67ms |
| `-Mode mem` | A02/A03/A04 私有工作集 / 三口径 / 句柄 | PASS（5.777 MB） |
| `-Mode months` | A06/A07 翻月 20 次内存与像素哈希往返 | PASS |
| `-Mode drag` | A08–A12 拖动位移 / 光标 / 禁拖区 | 12/12 PASS |
| `-Mode tray` | A13–A16 菜单可见 / 关闭 / 失焦消失 / 无残留 | PASS |

`-Mode idle10`（A05 静置 10 分钟增长）由 t4 复核。

## 已知环境限制（不是代码问题）

- embed 贴桌面层在本会话不可用：`FindWindowW("Progman")` 命中 `0x10196`，但
 `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 返回 `err=87`、读回 0；32 位
 `SetWindowLongW(GWL_HWNDPARENT, …)` 同样 0；`SetParent(hwnd, progman)` 返回 `err=5`（跨进程 reparent 被拒）。
 代码保留完整回退链并逐级记日志 `[embed] parent=… via=… prev=… err=… readback=… setparent=… longw=…`，
 失败时退到 `HWND_BOTTOM`（仍不置顶、不进 Alt-Tab）。用户交互桌面双击启动应可成功。
- `<root>\research\` 对本 exe 拒绝建文件（`os error 5`）：cmd/python/powershell 同目录写入正常，
 ACL / junction / `\\?\` 前缀均排除。持久化回归改用 `--state <root>\research2\widget_state.json`
 （`[state] saved … atomic=rename` 正常）。

## 数据层（t2）

两个文件，都不由本程序生成 schema，只做「读 + 容错 + 回写」：

`research/widget_data.json`（由每日管道产出，挂件只读）
除 `today/todayDay/week/ym/stamp/counts/todo/p0/nodes/cal` 外，新增 `signals`：今日高相关群消息，最多 3 条：

```json
"signals": [
  {"t":"2026-11-10 14:01","sender":"OWNER_E","text":"一句话原文","model":"ITEM_X"},
  {"t":"2026-11-10 13:40","sender":"OWNER_X·私聊","text":"…","model":""}
]
```

字段缺失一律按空处理，不会因为管道只写了一半就崩。

`research/widget_state.json`（读写，v2 结构）

```json
{
  "custom":     [{"id":"c20261110142530","text":"…","due":"2026-11-12","done":false}],
  "department": [{"id":"yPC_Todo_63929380150114","text":"…","due":"2026-11-04","done":false,"src":"yynote","importedAt":"2026-11-10T14:24:32"}],
  "done":       {"<model>|<name>|<日期>": true},
  "ui":         {"tab":"proj","win_mode":"embed","x":100,"y":200,"opacity":0.75}
}
```

规则：

- 整文件读入 修改 写回，未知字段（含 `ui` 段里的自定义键、顶层老字段）原样保留，不会被重建对象清掉。
- 老文件只有 `custom`/`done` 时自动补 `department: []` 与默认 `ui`，并立即落盘；数据不丢。
- `department` 从 `research/yynote_todos.json` 导入（`--yynote` 可覆盖，默认取 state 同目录）：新 id 追加，已存在 id 按 本地 done 优先 合并，只补文本/截止日。源文件不可读时 `department` 原样保留、不报错。
- 写入走 `<state>.json.tmp` + `sync_all` + `rename`，原子替换、UTF-8 无 BOM。
- 解析失败：原文件改名 `<state>.json.bad-<ts>` 备份，重建空结构继续运行，错误写 `research/widget_state_error.log`。

自测（`--selftest`）一次性覆盖：老格式迁移、原子写、无 BOM、UTF-8、`.tmp` 不残留、往返一致、取消勾选不残留 key。

采集调度：代码缺省白天 60 分钟、夜间 240 分钟；部署以 widget_config.json 的 30/60 为准。

挂件自己拉数据，不再靠外部定时器：

- 启动即抓取：`reason=startup`，进程起来就连跑一轮，页脚立刻显示「抓取中…」。
- 周期刷新：白天 60 分钟、夜间（22:00–08:00）240 分钟；`widget_config.json` 可覆盖
 （`{"pullMinutes":60}` 或 `{"dayMinutes":30,"nightMinutes":180}`），优先级
 CLI `--pull-minutes` > dayMinutes/nightMinutes > pullMinutes > 默认 60/240。
 `--pull-plan` 会把 24 小时逐一打印成表，便于核对。
- 不重叠：上一轮没结束（`WaitForSingleObject` 非 0）就只记 `[pull] wait reason=running`，不起第二轮；
 pid 文件 `widget_pull.pid` 记录在跑的 pid，进程已死则视为空闲。单轮超过 45 分钟由 `TerminateProcess` 收掉（watchdog）。
- 撞车跳过：`reason=schedule` 且当前时刻落在 09:10 ±3 分钟（计划任务跑同一条管道）时记
 `[pull] skip reason=0910-collision`，直接排下一轮；`reason=startup/manual` 不受影响。
- 拉起方式：`CreateProcessW` + `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`，cwd = 脚本目录，
 命令行 `pwsh -NoProfile -ExecutionPolicy Bypass -File "<每日项目简报.ps1>" -NoOpen -NoWidgetRestart`，
 子进程隐藏、无窗口、不继承控制台；输出由管道脚本自己写 `research\widget_pull.log`。
- 热重载：本轮 `exit 0` 后立刻重读 `widget_data.json`（`[data] reload after pull ok=true …`）+ 清画布缓存 + 整窗重绘，
 不需要等 30 秒 mtime 轮询。
- 页脚：左右卡底部固定一行「上次抓取 HH:MM · 下次 HH:MM」（抓取中显示「抓取中…」，禁用时「采集未启用 · 可手动刷新」）。
- 立即刷新：托盘右键第二项，走 `menu_reload()` 同一条路径：重读磁盘 + 触发一轮 `reason=manual`。
- 单实例：`CreateMutexW(Local\项目看板WidgetSingle)`，第二个实例记 `[single] another instance is running -> exit 0` 直接退出；
 验收/并发调试用 `--multi` 跳过。

验收日志（`evidence/pull-test/`）：`w.log`（启动即抓取 + 1 分钟周期 + 手动刷新）、`night.log`（假时钟 03:00 夜间 5 分钟）、
`t7-post.txt`（0x7FFE 钩子 `[menu] reload -> pull triggered` `[pull] start reason=manual`）、
`t7-single.txt`（A 拿锁、B 退出码 0）、`t7-deploy.txt`（部署 + 托盘 add ok=true + 页脚截图 `t7-footer.png`）。

## 实测指标（2026-11-10）

| 指标 | 值 |
|---|---|
| 私有工作集 `WorkingSetPrivate` | 5.777 MB（验收线 <15 MB，t1 骨架期 916 KB） |
| `WorkingSet`（含共享） | 9,656 KB |
| `PrivateBytes`（提交） | 1,296 KB |
| 峰值工作集 | 13.7 MB |
| 窗口尺寸 | 980×566（`GetWindowRect` 实测） |
| 依赖数 | 14（`cargo tree` 20 行，含根与 5 个构建期 proc-macro） |

`evidence/` 下是本次验收的原始记录：

- `verify-result.txt` — 扩展样式位、命中测试、内存采样、exe 大小
- `drag-result.txt` — 真实鼠标注入拖动（窗口 687,436 847,516，逐段 rect）
- `pixel-report.txt` — 角像素探针：`(1,1)/(4,4)` 透明（露出背景），`(12,12)/(3,60)/(200,60)` 为 `#12141A` 窗口底色，`ex_style_after_revert` 确认 TOPMOST=False
- `shot-window.png` / `corner-tl.png` / `shot-full.png` — 窗口截取、左上角 5× 放大、全屏截图
- `final-evidence.txt` — 收尾快照：exe 字节数、进程/主窗口句柄、扩展样式位、工作集、`cargo tree` 行数、拖动与内存记录汇总
rebuild-check.txt — 历史复检记录（非现役口径）：曾记 WM_NCHITTEST(5,5)/(500,300)=2（HTCAPTION）；现役以 :57 的 HTCLIENT 固定返回为准。
- `*.ps1` — 上述校验脚本（ASCII-only：PowerShell 5.1 按 ANSI 读无 BOM 的 .ps1，中文路径会乱码，脚本里统一用 `$PSScriptRoot`）

## 注意

- 本目录 `.cargo/config.toml` 固定了 MinGW 链接器路径；换机器要改这里。
- `target/` 是构建产物，不进版本库。
