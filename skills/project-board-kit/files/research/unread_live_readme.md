> 现役口径见 每日简报_使用说明.md（research\unread_snapshot.json）；本副本仅保留历史实现说明，不再保留 research\unread_snapshot.json 冲突描述。
# 未读消息 1 分钟级近实时（短连接，不顶号）

## 背景与硬约束
- E-Mobile 服务端对同一 udid（`<uid>|c`）只保留 1 个消息会话。
- 常驻 WebSocket 绑定会永久占位：2026-11-11 实测绑定后约 2 分钟桌面客户端被踢回登录页。
- 历史协议（v2.0 起废弃、会顶号）：`POST /emp/passport/login` 与 `GET /emp/api/msg/gettoken` 会重建会话，约 1 分钟后顶号；现役绝不调用。
- 伪装移动端（client_type=4/2/3）无效，msg_udid 仍为 `c`。
- 结论：唯一安全形态是「短连接 bind → 拉一次会话列表 → 立即 close」。绝不 login/gettoken、绝不常驻、绝不发历史 IQ、绝不与重管道并发绑 WS。

## 架构

### 1. `research/raw/emobile/em_unread_live.js` — 轻量短连接通道
- `--once`：XMPP SCRAM → bind → 只发一次 getConversation IQ → 写 `unread_snapshot.json` → 原子重写 `widget_data.json` 的 `signals`/`signalsMode` → `ws.close()` → `exit(0)`，硬超时 8s。
- `--render-only`：不连 WS，只按当前快照重算 signals，用于本地自检。
- 历史实现说明：`api/gettoken.json` 已是废弃产物；现役参数只复用本机客户端令牌与 `api/msgconn.json` 连接态。RESOURCE 每次随机 `ul` + hex，避免与桌面端撞 resource。
- 内置 `unreadSignals()` 与 `build_widget.mjs:214-261` 口径逐行一致（convKey 去重、isdel 跳过、48h 时效、排除自己发的、噪声过滤、最多 3 条）。
- 失败仅记 `research/unread_live.log` + 输出 `UNREAD_LIVE fail reason=...`，不抛异常、不重连。

### 2. `research/widget_config.json` — `unreadMinutes: 1`
- 重管道 `dayMinutes=30` / `nightMinutes=60` 不变。
- 轻量轮允许 `1/2/3/5` 分钟；`0` = 预留不启用。

### 3. `rust-widget/src/pull.rs` — `UnreadPuller`（与重管道 `Puller` 共存）
- 每秒 tick：先 `tick_heavy`（原重管道逻辑，函数改名平移），再 `tick_unread`。
- 撞车保护 `should_skip_unread()`：重管道正在跑（`heavy_running`）或同一分钟刚起过（`heavy-same-minute`）→ 跳过本轮轻量，记 `[unread] skip reason=... retry_in=15s`，15s 后重试，**绝不并发绑 WS**。
- 单实例：独立 `research/unread_pull.pid`，start 前检查 `process_name` 含 `node` 防并发。
- `UNREAD_MAX_RUN=30s` 看门狗；收尸后 `t.unread_finished=true` 触发挂件立刻重画。
- 常量：`UNREAD_MINUTES_DEFAULT=1`、`UNREAD_ALLOWED=[1,2,3,5]`。

### 4. `rust-widget/src/main.rs` — 立刻重画
- `tick()` 同时取 `(started, finished, unread_finished)`；`u_finished` 时立刻 `data::load` + `Draw::Full` 重画，日志标 `reload after unread-live`。
- 不等待默认 30s 的 `reload_secs` mtime 热载，保证新未读即时上屏。

## 上屏延迟
- 轻量轮每分钟一次，短连接实测约 136ms；新未读最迟 ≤60s 写入 `widget_data.json`。
- 挂件在 `u_finished` 时立刻全量重画，因此上屏延迟 ≈ 短连接完成时间，不叠加 30s reload 周期。

## 验证命令
- `node --check research\raw\emobile\em_unread_live.js`
- `node research\raw\emobile\em_unread_live.js --once` → `UNREAD_LIVE ok items=N ms=<10000`
- `CARGO_TARGET_DIR=<TEMP_BUILD> CARGO_INCREMENTAL=0 cargo test --quiet` → 13 passed
  - 注意：MinGW `<TOOLS>\mingw64\bin\gcc.exe` / `ld.exe` 无法解析中文 target 路径 `<ROOT>\rust-widget\target`，会报 `cannot find ...\symbols.o`。必须用纯 ASCII 的 `CARGO_TARGET_DIR`（如 `<TEMP_BUILD>`）。
- `CARGO_TARGET_DIR=<TEMP_BUILD> cargo build --release`
- 部署后：`项目看板.exe --version`、`Get-Process 项目看板`、7070 ESTABLISHED 计数、`widget_pull.log` 尾部 `[unread]` 行。

## 顶号回滚
若观测窗口内 7070 ESTABLISHED 掉到 0 或桌面客户端被踢：
1. 把 `research/widget_config.json` 的 `unreadMinutes` 从 `1` 改为 `3`（或 `0` 直接停用轻量轮）。
2. 挂件每 30s 读配置，无需重启即可降频/停用；重管道不受影响。
3. 在交付 output 中写明顶号事实与已回退值。
