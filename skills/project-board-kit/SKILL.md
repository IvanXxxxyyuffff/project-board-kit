# 项目看板套件 · 通用部署技能

> 目标：在一台新电脑上，从零部署一套「自动抓取企业微信文档 + 泛微 E-Mobile 项目群 生成项目数据看板 + 桌面挂件 + 每日简报」的系统。
> 读者：任何 AI 模型 / 工程师。**先看第 0 章（5 分钟部署），跑通后再看后面章节。**

---

## 0. 5 分钟部署（新用户只做这四步）

> 装完想确认「这个仓库本身是好的」，跑 `python scripts/selfcheck.py`，看到 `SELFCHECK_RESULT=PASS` 再往下。


前提：Windows 10/11，本机已登录企业微信 PC 客户端；Node 18+ 与 Python 3.10+（若没有，`discover.ps1` 会告诉你缺什么）。

```powershell
git clone https://github.com/<owner>/project-board-kit.git
cd project-board-kit\skills\project-board-kit
pwsh -File install.ps1 -Root <TARGET_DIR>\app
```

`install.ps1` 会依次做四件事，任一步失败都会明确告诉你原因：

1. **安装**：把套件复制到 `<TARGET_DIR>\app`，初始化 `research/config.json`，编译/放置采集器与挂件（无 Rust 工具链时自动从 Release 下载 `project-board-widget.exe`）。
2. **探测**：`discover.ps1` 找到 node / python / 企业微信 / E-Mobile 地址，写进 `research/config.json`。
3. **体检**：`doctor.ps1` 输出表格，`FAIL=0` 即达标；只剩「E-Mobile 口令未填 / 企业微信未登录 / 会议 Cookie 未采」三类环境项是正常的。
4. **给出下一步**：首跑命令、看板路径、定时任务命令。

部署完成后：

```powershell
pwsh -File "<TARGET_DIR>\app\每日项目简报.ps1" -NoOpen   # 首抓 + 生成看板，约 70 秒
start "<TARGET_DIR>\app\项目数据看板.html"                 # 打开看板
```

**没有 Rust / 不想编译**：不用管，安装脚本会用 Release 里的 `project-board-widget.exe`；想自己编译见第 6 章。

**装完挂件是旧版**：说明 exe 来源不对，跑 `doctor.ps1` 会直接报「挂件版本与源码一致 FAIL」，按提示替换为 Release 资产即可。

---

## 1. 这套东西产出什么

| 产出 | 路径（默认） | 说明 |
|---|---|---|
| 原始表格文本 | `research/wxdata/ALL_HTTP_TEXT.txt` | 两个腾讯文档共 19 个页签的纯文本，段头 `===== T1_05_000004 | 页签名 =====` |
| 群聊全集 | `research/raw/emobile/project_chatlog.json` | 136 群 / 2000+ 条，含 `sender`/`text`/`time` |
| 私聊筛选 | `research/raw/emobile/personal_chatlog.json` | 私聊里命中机型/关键词的消息 |
| 数据看板 | `项目数据看板.html` | 单文件，可点（KPI清单、热力当日明细、卡片抽屉） |
| 每日简报 | `最新项目简报.md` / `.html` | 供定时任务汇报 |
未读消息：现役口径取 `research\unread_snapshot.json`（--once 在拉历史前采集；按 `convKey||targettype:targetid` 去重、跳过 isdel、48h 内、排除自己发的、噪声过滤、最多 3 条）；状态灯 `runOk+authOk+scramOk` 三取一假即红。旧会话表未读计数口径已废弃。
| 完整性自检 | `research/数据完整性.json` | 页签数、文本行数、群数、消息数、告警数组 |

三条数据通道互相独立，任意一条断掉其余仍能出看板，只是对应区块标降级。

---

## 1. 先决条件（装机前确认）

1. Windows 10/11，能访问公司内网（E-Mobile 服务器）与外网（doc.weixin.qq.com）。
2. 企业微信 PC 客户端已登录（同一台机器）。这是微信文档通道的唯一凭据来源：客户端登录后 cookie 落在磁盘，我们会话外读取，不需要扫码、不需要人工开文档。
E-Mobile PC 客户端已登录（现役唯一前置；历史「或知道 E-Mobile 账号口令」口径已废弃）
4. Node.js ≥ 18（`node -v`）、PowerShell 7（`pwsh`）。
5. 可选：要做 exe 才需要 Rust（`rustup` + `x86_64-pc-windows-gnu` + MinGW-w64）；只想要网页看板可跳过。

---

## 1.5 通用边界：哪些自动适配，哪些必须按自己公司改

这套东西的抓取层是通用的（换电脑、换账号、换公司都能跑），解析层是给「一张项目进度表」定制的。
部署到别人机器前，先分清下面两类，不要一句「装不上」或一句「装上就能用」。

| 项 | 是否需要人工改 | 说明 |
|---|---|---|
| 安装目录、盘符、Windows 用户名 | 自动 | 根目录由 setup 写入；`wxwork_sheet_pull.js` 还会按自身位置自动上溯识别根目录，搬目录后用 `setup.ps1 -RepairPath` 一键改写 |
| E-Mobile 主机 / 端口 / 账号 / 口令 | 必须填一次 | `config.json` 的 `emobile.*`；口令单独存 `research/secrets.json` |
| 群、私聊、会议 | 自动 | 每轮重新拉 `getAllGroups` + `getConversations`，新群新私聊自动纳入；`targettype` 1=群 0=私聊 111=会议跳过 |
| 发言人姓名（含新入职） | 自动 | 抓完用 `msg/userinfo/simplelist` 反查真名，缺失才显示 `用户<id>` |
| 企业微信安装路径、账号目录（18 位数字） | 自动 | 扫 `userDataRoot` 下的数字目录；`exe`/`commandExe` 可在 config 指定 |
| 文档 cookie / 登录态 | 自动 | 从企业微信 cookie 库直取 + DPAPI 解密；`wxdoc_autorenew.ps1` 探活与自愈，失效写 `research\cookie_alert.txt` |
| 页签增删、改名 | 自动 | 每轮 `discoverSheets` 动态发现（`collab_client_vars.header[0].d[]` 里 `type==='grid'`），`sheet_diff.js` 报变化 |
| 定时时间 | 改 config 即可 | `schedule.dailyPull` / `dailyReport` / `weeklyCheckPatch` |
| 两个腾讯文档的地址 | 必须填 | `wxwork.docT1` / `docT2`，完整 URL；padId 从 URL 里的 `e3_xxx` 自动取，不需要单独填 |
| 表格列结构映射 | 必须改一次代码 | `research/build_daily_brief.mjs` 里按固定列序取值：机型 `c[2]`、品牌 `c[3]`、类型 `c[4]`、状态 `c[6]`、级别 `c[7]`、产品经理 `c[9]`（见 `:146` / `:273`）。换了表结构 = 抓取照样成功，但看板卡片会空 |
| 项目台账 `brief_focus.json` | 必须自己写 | 谁是 P0、卡点文案、下一节点、负责人，都是人工策展；套件里带的是示例 |
| 看板配色 / 字段名 / 优先级规则 | 可选 | 改 `build_daily_brief.mjs` 渲染层或 `autoPriority` |
| Rust 挂件 exe | 可选但推荐 | 源码在 `rust-widget/`，`cargo build --release` 即可；纯 alpha 自绘，内存约 8MB |

一句话：抓取链路（登录态、群、私聊、页签、cookie、定时）装上就能跑；
「看懂这张表」的部分要按你们自己的表改一次：列下标 + 台账，两处，改完即通用。

### 搬动安装目录之后

```powershell
pwsh -File .\doctor.ps1                                  # 会报「脚本根目录一致 FAIL」
pwsh -File setup.ps1 -RepairPath -Config .\config.json    # 把脚本里的旧根目录改写成当前目录
pwsh -File .\doctor.ps1                                  # 应变回 OK
```

`-RepairPath` 只改路径文本与 `config.json` 的 `root`，不动数据、不重装、不覆盖 `research\wxdata`。

## 2. 安装

> 顺序（务必按这个来）：`setup.ps1` `discover.ps1` `doctor.ps1` 首跑。
> `discover/doctor` 需要 `research/` 存在，而 `research/` 由 `setup.ps1` 从 `files/` 复制生成；先跑探测会提示「请先 setup」。

> 脚本来源：采集脚本（`research/raw/emobile/*.js|*.cjs`）随套件一起分发，`setup.ps1` 会把它们复制到 `<ROOT>\research\raw\emobile\`。
> 若你的来源包不含这些脚本，安装后 `doctor` 会列出缺失清单，按提示补齐即可（它们只是普通 Node 脚本，无需编译）。


安装顺序统一为：setup.ps1 → discover.ps1 → doctor.ps1 → 首跑 <root>\每日项目简报.ps1（顺序原因见本节上方引用块。）
# 2.1 把整个 project-board-kit 目录拷到目标机（例 `<TARGET_DIR>`）
copy config.example.json config.json # 2.2 填 4 个必填项（见下表）
pwsh -NoProfile -ExecutionPolicy Bypass -File .\setup.ps1 -Config .\config.json
pwsh -NoProfile -ExecutionPolicy Bypass -File "doctor.ps1"
```

`config.json` 必填项与**怎么拿到**：

| 字段 | 怎么拿 |
|---|---|
| `emobile.host` | 企业内网 IP。取法：E-Mobile 客户端「设置 → 服务器地址」，或 `netstat -ano | findstr :8999` 找已建连的对端 IP |
| `emobile.loginId` / `password` | **历史字段，v2.0 起不用**：现役只复用本机已登录客户端令牌，不要口令、不要新建会话 |
无需口令：先确认本机 E-Mobile 客户端已登录，再跑 em_login_state.js --auto --probe；历史登录成功日志口径已废弃。
| `wxwork.docT1/docT2` | 在企业微信里打开那两张表，复制地址栏 `https://doc.weixin.qq.com/sheet/e3_xxx?scode=xxx` |

setup.ps1 现做「基础五步 + 5b~5g」：建目录树、装预编译 exe、写 widget_config.json、注册计划任务、装 ws 依赖为基础五步；5b~5g 为数据/挂件/小黄条/进度图/自检动作。（5d–5g 是 setup 动作编号，与管道 5a V4 重建不是同一套编号。）

**验收**：`doctor.ps1` 期望输出（真实机器，`myUserId` 已填）：

```
config.json OK
node(>=18) OK v2x
pwsh 7 OK 7.x
E-Mobile 8999(HTTP) OK
E-Mobile 7070(WS) OK
企业微信进程 OK pid=...
微信文档 cookie OK [grab] size=... rows=30 CHECK cookies=13 ok=true
cargo(可选) WARN ...
待办源(可选) WARN ...
```

---

## 2.4 明确不要做的事（省时间）

- **不要让人扫码登录腾讯文档 / 不要装 EGO 浏览器**：微信文档通道是「读本机企业微信客户端的 cookie → 直连 dop-api」，纯 HTTP，无浏览器（详见 2.5.3）。
- **不要让人手工打开那两张表**：padId 由 `discover.ps1` 从企业微信缓存里自动挑出（按出现次数排序取前二），页签由 `discoverSheets()` 动态发现。
- **不要让人填服务器地址/账号**：E-Mobile 的 host/port/uid 从 `%APPDATA%\E-Mobile\User Files\http-<host>-<port>\<uid>\` 目录名反推。
- 唯一可能需要人给的输入：E-Mobile 口令（且只有客户端离线、内存/文件都拿不到令牌时才需要），以及他本人电脑上企业微信保持登录。
## 2.5 零人工：AI 自助获取登录态与数据源（推荐主流程）

目标：**装的人什么都不用填**。AI 按下面顺序跑，能自动拿到的全部自动拿，只在一处需要跟人确认一句话。

### 2.5.1 跑探测脚本（无参数，幂等）

```powershell
pwsh -File discover.ps1 # 自动定位安装根目录并写 research\config.json
pwsh -File discover.ps1 -Force # 重新探测并覆盖已写值
```

它会自动填好这些（都是本机事实，不需要人告）：

| 字段 | 怎么来的 |
|---|---|
| `paths.node` / `paths.pwsh` | `Get-Command` |
| `emobile.host` / `emobile.httpPort` | E-Mobile 客户端 userData 下的目录名 `User Files\http-<host>-<port>`（Electron 按服务器分目录，名字里就带地址） |
| `emobile.myUserId` / `authcid` | 该目录下的数字子目录（= 你的 uid），并拼出 `<uid>|c` 形式的 authcid |
| `wxwork.exe` / `commandExe` | 注册表卸载项 → 安装目录 → 运行中进程路径，三路兜底；`WXWorkCommand.exe` 在安装目录下递归找（带版本号子目录） |
| `wxwork.userDataRoot` | `%USERPROFILE%\Documents\WXWork`，并识别 10 位以上的账号目录 |
| `wxwork.docT1` / `docT2` | **扫企业微信数据目录里的 `e3_*` padId，按出现次数排序取前二** ： 你每天打开的那两张表次数会明显领先（实测：目标两张各 25 次，其余 12 次） |
| `wxwork.docCandidates` | 前 12 名候选，供人工/AI 复核 |

扫描规模受控：单文件 <6MB、最多 6000 个文件、`Latin1` 读字节（不会因非法 UTF-8 抛错）。

### 2.5.2 校验两个文档有没有选反

`docT1` / `docT2` 只是顺序占位，选反会让「已立项 / 预立项」标题对调：

```powershell
node research\raw\emobile\wxwork_sheet_pull.js
# 看 research\wxdata\wxwork_pull_stats.json 的 sheetsList[].name
# 若「已立项」出现在 T2 下 把 config.json 里 docT1 / docT2 两个 URL 对调即可
```

### 2.5.3 企业微信文档登录态：全自动，不要人扫码

`wxwork_sheet_pull.js` 每一步都不需要浏览器或扫码：

1. Restart Manager（`RmGetList`）问出真正占用 cookie 库的进程（`WXWorkWeb`），避免 `err=32`；
2. `DuplicateHandle` 复制句柄 → 读 `Network\Cookies` 副本；
3. DPAPI 解 `Local State` 的 `os_crypt.encrypted_key` 得 32 字节 AES key → 解 `v10/v11`（AES-256-GCM）；
4. 组 Cookie 头打 `dop-api`，`retcode=0` 即成功。

前提只有一条：**企业微信客户端处于登录状态**（进程在跑）。探活：`node research\raw\emobile\wxwork_sheet_pull.js --check`（退出码 0 = ok，2 = 登录态死）。
鉴权核心是 `wedoc_sid / sids / skey / ticket / openid / tdoc_uid`，到期通常是 14 天；`research\wxdoc_autorenew.ps1` 会在失效时拉起客户端并用 `WXWorkCommand.exe <文档URL>` 触发续期。

### 2.5.4 E-Mobile 登录态：复用本机客户端令牌（历史「口令是唯一开口」已废弃）

实测过三条「不用问人」的路，都不通，别再重复试：

- `%APPDATA%\E-Mobile\User Files\http-*\login.json` 只有 **20 字节**会话标记，无账号无口令；
- `sqlite3\em7.db` 有 `users` 表（列 `loginId/key/password/baseUserId`）但 **0 行**；
- Windows 凭据管理器（`cmdkey /list`）无 E-Mobile 条目；Electron Local Storage 里只有 `8999` 这类地址串。

所以按下表取口令，优先级从高到低：

| 情况 | 做法 |
|---|---|
| 同机之前配过 | `research\secrets.json` 已存在 → 直接复用，什么都不做 |
AI 不再问口令：现役只需确认本机 E-Mobile 客户端已登录，然后跑 `em_login_state.js --auto --probe`。历史「E-Mobile 登录密码是多少？」口径 v2.0 起废弃（会顶号）。
| 另一台已配好的机器 | 直接拷那个 `secrets.json`（同一域账号） |

em_contacts_refresh.js 打印 [FATAL] 缺口令 → 历史脚本，现役不要用；改跑 `em_login_state.js --auto --probe` 复用令牌。

### 2.5.4b 静默获取登录态（一条命令，零输入）

```bash
node research/raw/emobile/em_login_state.js --auto --probe
```

`--auto` 默认**只复用令牌、不登录**：`/passport/login` 与 `/msg/gettoken` 会让服务端重建消息会话，1 分钟内把在线桌面端顶下线。流程：

| 步 | 行为 | 结果 |
| --allow-login | **历史实验/已废弃**：曾以 client_type=4 伪装移动端并存；v2.0 起禁止任何新建会话，不要使用 |
| 1 | 复用 `api/msgconn.json` 已有令牌，连一次 7070 探 `stream:features` | 成功 → 退出 0（常态，有效期跨月） |
| 2 | 令牌不可用且未加 `--allow-login` | 打印「已存令牌不可用，且默认不登录（避免踢掉桌面客户端）」→ 退出 2，不碰服务端 |
| 3 | 加了 `--allow-login`（或 `EM_ALLOW_LOGIN=1`）但客户端 7070 在线 | 仍拒绝登录 → 退出 2 |
历史实验结论（已废弃）：加 --allow-login 且客户端离线时曾走凭证登录；现役一律不登录、只复用本机已登录客户端令牌。

实测（本机）：
> 注：以下 191–194 为 2026-11-12 历史实验样本；其中「凭证登录」口径 v2.0 起废弃，现役只认 --auto --probe 复用令牌。
- 凭证登录（`--allow-login` + 客户端离线）：`[auto] OK  access_token=185099…(len 32)  uid=<UID>` → `消息短令牌=djlo9k…(len 24)` → `ws://<host>:7070/ws/   authcid=<uid>|c` → `[PROBE] stream 建立成功`。
- 紧接着再跑一次（回落到复用）：`[auto] 复用已保存登录态 …（无需任何输入）` + `[PROBE] stream 建立成功`，零输入零延迟。
- 服务器地址也不用填：脚本从客户端目录名 `%APPDATA%\E-Mobile\User Files\http-<host>-<port>\` 反推 host/port。
- 内存令牌（`fromMemory()`）通道已跑通（3 个进程共 77,953 个候选串 → 筛出 4,283 个 32 位令牌 → 并发 16 校验 3.4 秒完），但**本机客户端当前是登出状态**（`User Files\...\system.json` 里 `isLogout:true`），所以那一轮 0 命中；客户端登录时它才会命中。

想确认客户端到底登没登：解密 `system.json`（见 3.2.1）看 `isLogout`，不要凭「进程在跑」判断。

### 2.5.5 口令缺失时的降级（允许先跑起来）

只有企业微信文档、没有 E-Mobile 也能出看板：项目卡片、节点、看板 HTML、挂件全部正常，**缺的是群聊信号与「未读消息」**。`每日项目简报.ps1` 会把这步标 `SKIP` 继续走完，`数据完整性.json` 的 alerts 会提示 `E-Mobile 未授权`。

### 2.5.6 完整顺序

```powershell
pwsh -File discover.ps1 # 自动探测写配置
pwsh -File doctor.ps1 # 体检（唯一 PASS/FAIL 列表）
node research\raw\emobile\wxwork_sheet_pull.js --check # 文档登录态
pwsh -File 每日项目简报.ps1 -NoOpen # 端到端
pwsh -File research\register_daily_task.ps1 # 工作日 09:05 定时
```

## 3. 通道 A：泛微 E-Mobile（项目群 + 私聊）

### 3.1 协议事实（历史协议考古；v2.0 起两步 login 已废弃、会顶号，现役见 2.5.4b / 第十七节）

- 消息服务是**独立端口 7070**，路径 `/ws/`，WebSocket 子协议 **`xmpp`**。它不是 8999 的代理，两个都要开。
- 认证只有 **SASL SCRAM-SHA-1**（`PLAIN` 会返回 `invalid-mechanism`，老的 `iq-auth` 返回 401，都已判死）。
- 历史协议事实（v2.0 起废弃、会顶号）：旧版口令密文曾来自 `POST /emp/passport/login`，WS 口令曾取自 `GET /emp/api/msg/gettoken`；现役只复用本机已登录客户端令牌，禁止再走这两步。
- 握手顺序**不能乱**：`<open>` → `<auth>` → `<challenge>` → `<response>` → `<success>` → **再发一次 `<open>`** → `<bind>` → `<session>`。少了第二次 `<open>` 后面全部无效。

### 3.2 登录态是怎么拿到的（历史协议考古：两步 HTTP 登录已废弃、会顶号，不要执行）

被测环境：`host=<你的 E-Mobile 服务器>:8999`、`loginid=<工号>`、`uid=<你的 uid>`（下文实测输出已遮蔽）。

**第 1 步 · 账号登录 → access_token**

```
[历史协议/已废弃] POST http://<host>:8999/emp/passport/login
Content-Type: application/json
emclienttag: em7

{"loginid":"<工号>","password":"<口令>","captcha_text":"","key":""}
```

返回 `access_token`（实测 32 字符）+ `base_user_id`（就是 uid）。
失败时：HTTP 非 200 或 body 里带 errmsg（口令错 / 需要验证码 captcha_text / 账号被锁 / host 写错）。
这个 token 用于 8999 的消息类接口，headers 三个都要带：`access_token`、`emaccesstk`、`userheaderuserid`。
**它不能直接当 WS 口令**。

**第 2 步 · 换「消息登录态」→ 消息短令牌**

```
[历史协议/已废弃] GET http://<host>:8999/emp/api/msg/gettoken?getTime=<毫秒时间戳>
access_token: <第 1 步>
emaccesstk: <第 1 步>
userheaderuserid: <uid>
```

返回的是消息服务全套配置，其中五项决定 WS 怎么连：

| 字段 | 实测值 | 作用 |
|---|---|---|
| `消息短令牌` | 24 字符 | **SASL SCRAM-SHA-1 的口令** |
| `msg_user_id` | `<uid>` | authcid 前半段 |
| `msg_udid` | `c` | authcid 后半段 |
| `msg_domain` | `weaver` | `<open to="weaver">` |
| `pc_api_ip` / `pc_port` | `<host>` / `7070` | 拼 `ws://<ip>:<port>/ws/` |

`authcid = msg_user_id + "|" + msg_udid`（形如 `<uid>|c`）。

本机实跑输出（原样）：

```
[历史日志/已废弃] [1/2] POST /emp/passport/login host=<host>:8999 loginid=<工号>（v2.0 起禁止，会顶号）
 OK access_token=<32 字符> base_user_id=<uid>
[历史日志/已废弃] [2/2] GET /emp/api/msg/gettoken（v2.0 起禁止，会顶号；现役只复用本机客户端令牌）
 OK 消息短令牌=<24 字符>
 ws ws://<host>:7070/ws/ authcid=<uid>|c domain=weaver
 版本 msg_server_version=7.1.20260317 pc_port=7070 app_port=5222
 历史产物（已废弃）：api/login.json / api/gettoken.json / api/msgconn.json，仅用于辨认旧日志
[PROBE] stream 建立成功（下一步：SASL SCRAM-SHA-1） 用时 15ms
```

**一键脚本**（两步全做，可选再探一次 WS）：

```bash
node research/raw/emobile/em_login_state.js # 取登录态
node research/raw/emobile/em_login_state.js --probe # 顺手验证 7070 能不能握手
```

- 历史凭证来源（v2.0 起废弃）：环境变量 `EM_LOGINID` / `EM_PASSWORD` → `research/secrets.json` 的 `emobile.loginId` / `password` →（旧安装）`--from` 从 `em_contacts_refresh.js` 正则抽取；现役不读口令，只复用本机已登录客户端令牌。
- 历史产物（已废弃）：`api/login.json`、`api/gettoken.json`、`api/msgconn.json`（仅用于辨认旧日志；现役读本机客户端令牌与 msgconn 连接态，不再各自拼）
- 历史脚本退出码（旧两步登录脚本，现役不用）：`0` 成功 / `2` 缺凭证 / `3` 登录失败 / `4` 旧 gettoken 失败 / `5` 网络不可达 / `6` probe 超时 / `7` WS 连不上
- `--probe` 只握到 `<open>` 收到 `stream:features` 就退出，用来判断「服务在、端口没换」

**最容易踩的区分**

| 用途 | 凭证 | 寿命 | 失效怎么办 |
|---|---|---|---|
| 8999 HTTP 业务接口（`simplelist`、通讯录快照） | `access_token` | 每次登录换新，小时级 | 重新 POST login |
| 7070 WS SASL 口令 | `消息短令牌` | 长期稳定（实测跨月未变） | 重新走第 2 步 |

两者都**不需要**客户端 cookie，也**不需要**读内存。

**为什么不复用客户端已登录会话**：E-Mobile 把口令存在 `%APPDATA%\E-Mobile\sqlite3\em7.db` 的 `users` 表（结构里确有 `password`/`key` 字段，但实测 **0 行**）和 `User Files\http-*\login.json`（仅 20 字节会话标记），`cmdkey /list` 里也没有条目 ： 拿不到可用口令。所以「登录态」= 用一次口令换出的 `消息短令牌`，之后长期复用；真正会过期的是 8999 的 `access_token`，脚本每次跑自己换。

### 3.2.1 客户端本地文件是 hex + AES-256-CTR（可自查，别指望里面有口令）

`%APPDATA%\E-Mobile\User Files\` 下的 JSON 全部是密文，算法在客户端 `resources/app.asar` 的 `src/lowdb/util/cryptr.js`：

- 算法 `aes-256-ctr`，**key = `sha256("weaveremobilepc")`**（客户端常量，不是用户口令）
- 密文 = 前 16 个 hex 字符当 IV（按 utf8 取 16 字节）+ 其余 hex 为 ciphertext

```js
const KEY = require('crypto').createHash('sha256').update('weaveremobilepc').digest();
const d = require('crypto').createDecipheriv('aes-256-ctr', KEY, Buffer.from(s.slice(0, 16), 'utf8'));
JSON.parse(d.update(s.slice(16), 'hex', 'utf8') + d.final('utf8'));
```

解出来能看到：`base.json`=服务端登录页配置、`<uid>/draft.json|files.json|images.json|search.json`=业务缓存、`<uid>/system.json`=`{isLogout, winSize, downloadAddress, ...}`。
**里面没有口令、也没有可用令牌**（`Cookies` 表 0 行、`sqlite3/em7.db` 的 `users` 表 0 行），所以第 3 级才需要扫内存。

### 3.3 拿全量消息（WS）

```bash
node research/raw/emobile/em_full_pull.js --fresh # 首轮全量（7 波左右，约 3–5 分钟）
node research/raw/emobile/em_full_pull.js # 之后增量（约 20–60 秒）
```

内部机制（改脚本时别破坏）：

| 环节 | 关键点 |
|---|---|
| 会话发现 | `iq id=getAllGroups` + `getConversation`，`targettype` 分流：`1`=群、`0`=私聊、`111`=会议（跳过） |
| 私聊 | **历史拉取必须传 `targetType:1`**；传 0 返回全空（踩过） |
| 翻页 | 每会话 `lastTime` 时间游标 + 页大小 50；**连续 2 页零新增才收敛**（`ZERO_PAGES=2`），`MAX_WAVES=80` |
| 限速 | 业务 IQ 之间 220–240ms，否则服务端断链 |
| resource | 必须用**独立 resource**（`pull*`/`hist*`），用客户端占用的 `pc` 会 `CLOSE 1006` conflict |
| 去重 | 优先 `opBody.msgid`，无则 `群+时间+发送人+文本` 哈希 |

> ⚠️ **并发陷阱**：一次灌 12 条业务 IQ 会让服务端直接 `CLOSE 1006`（表现为 `responses=0`）。凡探针类脚本，必须**串行 + 每条间隔 ≥1.2s**。

### 3.4 姓名解析（把「用户<id>」变成真名）

```bash
node research/raw/emobile/em_names_by_id.js # 无参 = 自动找 project_chatlog 里没映射的发言人
```

**关键**：`group` 命名空间下的 `getGroupMembers` / `getGroupInfo` / `getMembers` 等 **一律返回 `type=error`**（服务端不开放）。能用的只有消息命名空间的：

```
POST http://<host>:8999/emp/api/msg/userinfo/simplelist
body: {"userlist":[475,650],"grouplist":[],"agentlist":[],"tenantlist":[]}
headers: access_token / emaccesstk / userheaderuserid
 {"errcode":0,"userlist":[{"id":"475","name":"张三","position":"包装组长",...}]}
```

这条不需要组织架构权限。产物 `names_by_id.json`，`build_chatlog.js` 会自动合并。

### 3.5 生成聊天记录

```bash
node research/raw/emobile/build_chatlog.js # project_chatlog.json / .md / users_map.json / personal_chatlog.json
node research/raw/emobile/em_unmapped_report.js # research/unmapped_report.json（未识别发言人占比）
```
验收：`UNMAPPED` 里只剩 `admin`（系统号）。真人未识别应为 0；如果不是，说明 3.4 没跑或 `names_by_id.json` 被清空。

---

## 4. 通道 B：企业微信文档（两张项目管理表）

**原理**：不扫码、不开浏览器。企业微信客户端登录后，cookie 加密存在磁盘的 SQLite 里；我们从**持锁进程**里复制出文件句柄读出来，DPAPI 解主密钥，AES-256-GCM 解每条 cookie，然后直接打腾讯文档的 `dop-api`。

### 4.1 抓 cookie

```powershell
# setup.ps1 已尝试自动编译；没成功就手动：
csc /nologo /out:"<root>\research\wxwork_handle.exe" "<root>\research\raw\emobile\wxwork_handle.cs"
& "<root>\research\wxwork_handle.exe" # research\wxwork_cookies.json
```
脚本自己也支持三个位置找 exe：`research\wxwork_handle.exe`、`.cs` 同目录、`research\raw\emobile\wxwork_handle.exe`。

四步原理，出问题时按这个查：
1. **找持锁进程**：cookie 库被 `WXWorkWeb.exe` 独占，`CreateFileW` 直接开报 `err=32`。用 **Restart Manager `RmGetList`** 问出真正持锁的 pid（`RM|HOLDER|pid=...|app=WXWorkWeb`），再 `OpenProcess` + `DuplicateHandle` 复制句柄读内容。
2. **解主密钥**：`Local State` 的 `os_crypt.encrypted_key` 是 base64，去掉前 5 字节 `DPAPI` 后 `CryptUnprotectData` → 32 字节 AES key。
3. **解 cookie**：`v10`/`v11` 前缀 → AES-256-GCM，`nonce`=3..15，`ciphertext`=15..len-16，`tag`=末 16 字节。
4. **拼 Cookie 头**：只取 `doc.weixin.qq.com` 的域，拼成 `k=v; k=v`。

**登录态能撑多久**：核心 6 条 `wedoc_sid/wedoc_sids/wedoc_skey/wedoc_ticket/openid` + `tdoc_uid` 同到期，实测 14 天。客户端只要在线会自动续期，所以每日管道第一步先探活、必要时拉起 `WXWork.exe` + `WXWorkCommand.exe "<文档URL>"` 让它自己刷新。

### 4.2 拉表格

```bash
node research/raw/emobile/wxwork_sheet_pull.js # 一条命令：重抓 cookie 动态发现页签 拉全部页签
node research/raw/emobile/wxwork_sheet_pull.js --check # 只探活（退出码 0=ok / 2=登录态死）
```

端点与坑：

| 项 | 值 |
|---|---|
| 探活 | `GET /dop-api/opendoc?id=<docid>&scode=<scode>&normal=1&outformat=1&noEscape=1`，看 `cgicode`：`0` 正常；`30004` 登录态死；`200060` 未登录 |
| 取单元格 | `GET /dop-api/get/sheet?padId=<e3_...>&subId=<页签id>&startrow=0&endrow=-1&xsrf=&outformat=1&normal=1&preview_token=` |
| padId | **必须是真 padId**（`e3_` 开头 32 字符）。用错返回 `{"retcode":200000}` 但 **HTTP 仍是 200**，只能看 retcode |
| 页签清单 | 从 opendoc 的 `clientVars.collab_client_vars.header[0].d[]` 过滤 `type==='grid'`；**不要写死**，页签会增删（实测有个页签被删后 `get/sheet` 返回 `retcode:ITEM_X invalid sheetID`） |
| 解压 | 响应里多个 base64 载荷：`workbook` 只是页签元数据，**单元格在 `related_sheet` 等后续载荷**；`zlib.inflate` 失败就试 `inflateRaw` / `gunzip` |
| 抽文本 | 只做宽松 UTF-8 可读串提取（长度≥2）；严格的 protobuf wire scan 会全空 |

产物 `research/wxdata/wxwork_pull_stats.json` 里有 `sheetsList`（页签 id+name），供 4.3 做变动检测。

### 4.3 变动自检

```bash
node research/sheet_diff.js # 页签增/删/改名，对比 research/sheets_snapshot.json
node research/project_diff.js # 表格机型 vs 看板机型，报「表里有看板没有」「新出现未收录」
```
两个脚本都自建基线，每轮把当轮清单写成新基线。**页签新增会自动被抓**（4.2 是动态发现），告警只是提醒你把这个项目补进策展台账。

---

## 5. 生成看板 / 挂件 / exe

```bash
node research/build_daily_brief.mjs # 数据层 + 渲染层 项目数据看板.html + 最新项目简报.md
node research/build_widget.mjs # 桌面挂件.html + research/widget_data.json（挂件数据）
```

**策展台账**（必须人工维护，是「项目全集」的稳定来源）：`research/brief_focus.template.json` → 拷成 `research/brief_focus.json`。每项：

```json
{"model":"ITEM_X","priority":"P0","owner":"张三","standalone":true,
 "blocker":"客户觉得贵，竞品 2~2.5 美金","next":"投模等客户老板决策",
 "action":"催价格复核","milestones":[{"name":"投模","plan":"11/18"}]}
```
`standalone:true` 表示「不在两个汇总页签里」，生成器会按台账兜底渲染 ： 少了它会整行消失（踩过：ITEM_X、ITEM_X 都因此丢过）。

**Rust exe（可选，内存占用 ~8MB 私有工作集）**：

```bash
cd rust-widget
rustup target add x86_64-pc-windows-gnu
cargo build --release # 产物 target/release/*.exe，移到 tools\项目看板.exe
```
exe 直接读 `research/widget_data.json` + `research/widget_state.json`（`custom[]` 自建待办 / `department[]` 从待办软件同步 / `archive[]` 归档 / `ui{tab,win_mode}`）。数据文件变了它会热重载。

采集：exe 启动即跑一次 `每日项目简报.ps1 -NoOpen -NoWidgetRestart`，之后按 `research/widget_config.json` 的 `dayMinutes=30` / `nightMinutes=60` 自跑（22:00–08:00 走夜间档；日志 `research/widget_pull.log`；`--no-pull` 可关）。源码默认是 60/240，config 优先。

**挂件交互**（Rust 自绘，不用 WebView）：

| 操作 | 结果 |
|---|---|
| 段式标签 | 「项目进度 / 部门进度」，部门进度内含「归档」子视图 |
| 优先级胶囊 | P0 红 / P1 琥珀 / P2 蓝，常显 |
| 行尾倒计时 | 逾期 N 天=红 · 今天=橙 · 明天=橙淡 · 其余=N 天灰 |
| 点日期格 | 左卡切成当天节点清单，再点日历别处恢复 |
| 拖拽待办行 | 行浮起跟随光标、落点画插入线、其余行让位；松开写回 `widget_state.json` |
| 双击待办行 | 编辑弹窗（部门同步项 / 自建项都支持） |
| 点勾选圈 | 勾上即沉归档（`reason=done`），不留删除线 |
| 点行尾 × | 软删入归档（`reason=deleted`），3 秒内二次确认；彻底删除同样二次确认 |
| 齿轮 | 不透明度 40%–100%（步进 5%）、固定位置（锁上禁拖窗口） |
| 拖窗口边角 | 等比缩放，最小 980×566、最大 1900×1200 |
| 托盘右键 | 系统原生菜单五项：显示/隐藏 · 立即刷新 · 打开看板网页 · 开机自启（勾选态）· 退出 |

---

## 6. 端到端 + 定时

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\每日项目简报.ps1 -NoOpen
期望：抓取轮 12 项 + 渲染轮 3 项（根版 15 Add-Step，套件补齐 5a 后 14/15），完整性自检以 `research\数据完整性.json` 的 `alerts=[]` 为准；总耗时 60–80s（目标 <300s）。
```

管道顺序：cookie 续期 → 微信表格 19 页签 → 页签变动 → 机型对齐 → E-Mobile 增量 → 发言人姓名 → 重建聊天 → 5a 重建 V4 看板(sheet_parse→sheet_canon→inject_sheet_truth→_ring_fix3) → 看板(最新项目简报.html → V4 现役页 项目数据看板.html) → 挂件。

定时：

```powershell
pwsh -File scripts\register_daily_task.ps1 # 工作日 09:05 计划任务 SELF_CO-Daily-Brief
```
再加一个会话/机器人定时：**工作日 09:10** 读 `最新项目简报.md` 汇报（不要重跑抓取）；**每周五 09:10** 检修（抓取成功率 / 数据完整性 / 台账漏项 / cookie 剩余天数 / 挂件 10 分钟稳态 `research/soak_check.ps1`），产出 `检修报告_<日期>.md`。

---

## 7. 配置项总表（`research/config.json`）

| 键 | 含义 |
|---|---|
| `root` | 安装根目录 |
| `emobile.host` / `wsUrl` | E-Mobile 服务器；WS 地址 |
| `emobile.loginId` / `password` | E-Mobile 账号口令（口令同时在 `research/secrets.json`） |
| `emobile.myUserId` / `authcid` | 登录后的 `base_user_id`；SCRAM 的 `authcid` = `<uid>|c` |
| `wxwork.exe` / `commandExe` | 企业微信客户端与命令转发器 |
| `wxwork.userDataRoot` | `Documents\WXWork`，cookie 库在其下 `*\WXWorkCefCache\Network\Cookies` |
| `wxwork.docT1` / `docT2` | 两张表的浏览器地址 |
| `schedule.*` | 抓取 / 汇报 / 检修时间 |

---

## 8. 安全约束（必须遵守）

- 口令只落 `research/secrets.json`（ACL 限本人），**不写进脚本、不进日志、不进看板 HTML**。
- cookie 只读不写；抓 cookie 用 `SQLITE_OPEN_READONLY` 语义 + 只读复制，**不修改客户端数据**。
- 看板生成器有断言：`{{...}}` 占位符与 token 形状出现即报 `tokenLeak`（`research/assert_status.json`）。
- 所有抓取幂等：重复跑不产生重复消息（按 `msgid` 去重）、不重复写待办（按 `id`）。

---

## 9. 排错表（错误原文 → 原因 → 修法）

| 现象 | 原因 | 修法 |
|---|---|---|
| 装完 `doctor.ps1` 报 `E-Mobile 8999/7070` 或 `config.myUserId` 为 WARN | 首次安装还没填 `em.host` / 没登录过 | 属正常：填 `em.host` 后跑 `em_contacts_refresh.js`，再跑一次 doctor 应变 OK |
| `微信文档 cookie` 提示「本套件不含 cookie 读取器」 | 公开套件不含 `wxwork_handle.exe/.cs`（内部树提供） | 该通道停用即可；E-Mobile 群聊与企微未读两条通道不受影响 |
| `CLOSE 1006 responses=0`（WS 探测） | 并发灌多条 IQ | 串行 + 每条间隔 ≥1.2s |
| `CLOSE 1006` 且 bind 后立刻断 | resource 与在线客户端冲突 | 换独立 resource（`pull*`） |
| `SASL failure` / `invalid-mechanism` | 用了 PLAIN 或旧 iq-auth | 只走 SCRAM-SHA-1，口令用 `消息短令牌` |
| `getGroupMembers` 返回 `type=error` | 该命名空间不开放 | 改用 `simplelist`（3.4） |
| `get/sheet` → `ITEM_X invalid sheetID` | 页签被删/换 id | 动态发现页签（4.2），别写死 |
| `retcode 200000 get pad meta error` | padId 错（用了文档 id 而非 `e3_...`） | 从 opendoc 的 `clientVars.localPadId` 取 |
| `retcode 30004 userinfo is nil` | cookie 死了 | 跑 `wxdoc_autorenew.ps1` 拉起客户端续期 |
| `CreateFileW err=32` | cookie 库被占用 | Restart Manager 找持锁 pid + DuplicateHandle（4.1） |
| 抽出的文本全是乱码/空 | 用了严格 wire scan | 改宽松 UTF-8 可读串提取 |
| 看板整块空白、`base_valid` 相关 | Rust 首帧没建静止层 | 脏区重画遇 base 未建先升格 Full |
| 双击不打开看板 | 缺双击分支 | `GetTickCount64` 手动判定 350ms/4px，**别加 `CS_DBLCLKS`**（会让三连击回归） |
| 脚本报 `Unexpected token` | `.ps1` 存成无 BOM，PS5.1 按 ANSI 读 | 统一用 pwsh 7；或补 BOM |
| `EXECUTOR_UNAVAILABLE ... awk.exe` | 某 bash 执行的 link-count bug | 换 `cmd /c` / pwsh 7 直跑 |
图表色块/圆环不显示 | 不要回退 CSS conic-gradient | 现役 `research\_ring_fix3.py` 把 conic-gradient 解析成 SVG `<circle stroke-dasharray>` 弧（viewBox118 / r=53.5 / 减 0.45 / data-ring 幂等）
| `intercepts pointer events` | 长驻浏览器标签页 hit-test 陈旧 | 新建 `newContext()` 再测 |

---

## 10. 验收清单（部署完逐条打勾）

- [ ] `doctor.ps1` 全 OK
验收清单：`em_login_state.js --auto --probe` → OK / [PROBE] stream 建立成功；不再列旧通讯录刷新脚本的登录成功日志（已废弃）。
- [ ] `em_full_pull.js --fresh` → `END ALL` 且 `groups ≥ 100`
- [ ] `em_names_by_id.js` → `FOUND n`（n 可为 0，表示没有新的未识别）
- [ ] `build_chatlog.js` → `unmapped` 只剩 `admin`
- [ ] `wxwork_sheet_pull.js --check` → 退出码 0
- [ ] `wxwork_sheet_pull.js` → `sheets=19/19`
- [ ] 管道一次跑通 <300s，`数据完整性.json` `alerts: []`
- [ ] `项目数据看板.html` 打开可点：KPI 卡弹清单、热力柱出当日明细、卡片开抽屉
- [ ] 计划任务 `Ready`，下次触发时间正确
- [ ] （可选）`项目看板.exe` 私有工作集 <15MB，双击卡片能开看板

---

## 11. 目录结构（安装后长这样）

```
<root>\
每日项目简报.ps1 ← 抓取轮 12 项 + 渲染轮 3 项（根版 15 Add-Step，套件补齐 5a 后 14/15），完整性自检 alerts=[] 为准
 每日简报_使用说明.md
 doctor.ps1 ← 环境自检
 SKILL.md ← 本文件
 research\
 config.json ← 由 setup 生成（运行时读）
 secrets.json ← 口令，单独文件
 brief_focus.template.json ← 拷成 brief_focus.json 再填
 build_daily_brief.mjs build_widget.mjs build_icon.py 项目看板.ico
 sheet_diff.js project_diff.js sync_yynote_todos.mjs
 wxdoc_autorenew.ps1 wxdoc_pull_all.mjs register_daily_task.ps1
 soak_check.ps1 ← 挂件 10 分钟稳态自检（周五检修第 5 项）
 widget_config.json ← 挂件采集间隔 {dayMinutes:30, nightMinutes:60}
 widget_pull.log ← 挂件自跑管道的日志（运行后生成）
 wxwork_handle.exe ← setup 自动编译
 raw\emobile\
 em_contacts_refresh.js(历史,现役不用) em_full_pull.js em_names_by_id.js
 em_unmapped_report.js build_chatlog.js wxwork_sheet_pull.js
 wxwork_handle.cs
 api\ ws_histories_full.json project_chatlog.json ... ← 运行后生成
 wxdata\
 ALL_HTTP_TEXT_WXWORK.txt wxwork_pull_stats.json ... ← 运行后生成
 rust-widget\ ← 可选：cargo build --release
 tools\ 项目看板.exe ← 可选：从 rust-widget\target\release 拷过来
```

**为什么保持这个结构**：所有脚本里的 `__dirname` 相对路径、`research\...` 绝对路径都是按它写的。换结构就得改脚本，别动。

## 12. 一分钟自检（装完/改完必跑）

```powershell
pwsh -File "<root>\doctor.ps1" # 期望：只剩 cargo / 待办源 两条 WARN
node "<root>\research\raw\emobile\wxwork_sheet_pull.js" --check # 期望退出码 0
期望：抓取轮 12 项 + 渲染轮 3 项，<300s，`research\数据完整性.json alerts=[]` 为准（旧步数口径已废弃）。
```

已在本机实测通过的判据（可作为「装好了」的证据）：
- `doctor` → `微信文档 cookie OK [grab] size=28672 rows=30 → CHECK cookies=13 ok=true`
- 历史样本（已废弃）：`em_full_pull.js --fresh` → `groups=136 totalMessages≈2000 newMessages>0`；现役以 69.2s 与 alerts=[] 为准
- `build_chatlog.js` → `unmapped` 只剩 `admin`
现役基线（2026-11-13）：DONE 69.2s | 页签 19 · 群 282 · 消息 2169；数据完整性.json sheets=19、httpLines≈27636、alerts=[]。历史 sheets 19 / httpLines ≈7960 / groups 282 / messages ≈2000 为旧样本。

> 修订 2026-11-12：桌面挂件标签更正为「项目进度 / 部门进度」，补未读消息口径。
> 修订 2026-11-12：2.5.4b 改为默认只复用令牌、不登录（登录会把在线桌面端顶下线）。
> 修订 2026-11-12：Rust exe 段补挂件交互表与 30/60 分钟采集间隔。
> 修订 2026-11-12：计划任务名更正为 SELF_CO-Daily-Brief，周五检修补第 5 项 soak。
> 修订 2026-11-12：目录树补 soak_check.ps1、widget_config.json、widget_pull.log。

## 十七、安装即用：登录态 / 数据 / 挂件 / 小黄条待办（2026-11-12）
> 5d–5g 是 setup 动作编号，与管道 5a V4 重建不是同一套编号；本节现役登录态只复用本机已登录客户端令牌（em_login_state.js --auto --probe）。

装完 `setup.ps1` 后**不需要再手工配置**，下列四件事会自动完成或一键完成。

### 17.1 装完立刻有的东西

| 产物 | 位置 | 谁生成 |
|---|---|---|
| 看板挂件 exe | `<root>\tools\项目看板.exe`（预编译，免装 Rust） | `setup.ps1` 5d |
| 开机自启 | `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\项目看板.lnk` | `setup.ps1` 5e |
| 部门待办（小黄条导入） | `<root>\research\widget_state.json` 的 `department[]` | `setup.ps1` 5f + 每轮管道 4b |
| 挂件进程 | 已启动（贴屏幕底部，托盘有图标） | `setup.ps1` 5g |

挂件行为：双击任意正文/项目行/日期格 → 打开 `<root>\项目数据看板.html`；右键托盘 → 隐藏/刷新/打开网页/开机自启/退出；拖标题或面板空白处移动窗口；拖右下角等比缩放（最小 980×566）。

### 17.2 E-Mobile 登录态（零输入优先，按顺序自动尝试）

`research\raw\emobile\em_login_state.js --auto` 依次试四级，**任何一级成功就停**：

1. **复用**已保存的 `research\raw\emobile\api\msgconn.json`，并用完整 SCRAM-SHA-1 真验证（不是看 `stream:features`）；
2. **历史凭证来源（已废弃）**：环境变量 `EM_LOGINID`/`EM_PASSWORD` → `research\secrets.json` → 旧安装的 `em_contacts_refresh.js` 内联常量；v2.0 起不走这一步，避免新建会话顶号；
内存借令牌（历史描述，已改口径）：em_memscan.exe 扫本机 E-Mobile 客户端内存取已登录令牌；不再逐个打 `GET /emp/api/msg/gettoken` 校验，该接口 v2.0 起废弃（会顶号）。
全失败时绝不新建会话：`POST /emp/passport/login` 与 `GET /emp/api/msg/gettoken` 会顶下线；处理方式是让用户打开 E-Mobile 客户端并保持登录，再跑 --auto --probe。

给同事的一句话操作：**把 E-Mobile 客户端打开并保持登录，然后跑 `node research\raw\emobile\em_login_state.js --auto --probe`**。服务器地址无需填 ： `detectServer()` 从 `%APPDATA%\E-Mobile\User Files\http-<host>-<port>\` 目录名反推。

只有客户端从未登录过才需要人工介入：请先打开 E-Mobile 客户端并保持登录；不要把口令写进 `research\secrets.json` 当现役步骤（历史口径已废弃）。

### 17.3 企业微信文档登录态（cookie 直连，无需扫码）

`research\raw\emobile\wxwork_sheet_pull.js` 自带：Restart Manager 找持有者 → `DuplicateHandle` 复制文件句柄 → DPAPI 解 `Local State` 的 `os_crypt.encrypted_key` → AES-256-GCM 解 `v10/v11` cookie → 调 `/dop-api/get/sheet`。

前置：**企业微信客户端在运行且已登录**（cookie 存在客户端本地库里）。失效判定看 `retcode`（`30004`=未登录、`200000`=padId 错、`ITEM_X`=subId 无效），**HTTP 200 不代表成功**。续期由 `research\wxdoc_autorenew.ps1` 自动做（失效时拉起 `WXWorkCommand.exe <文档URL>` 让客户端自己续）。

### 17.4 小黄条待办 → 部门进度

`research\yynote_import.ps1`：
1. 找库：`config.yynote.db` → `%LOCALAPPDATA%\YYNoteVip\yynote.db` → `%USERPROFILE%\Documents\YYNote\yynote.db`；
2. 找原生库：`<ROOT>\YYNote\runtimes\win-x64\native\e_sqlcipher.dll`（32/64 位两处都试）；
3. `yynote_export.ps1` 以 **SQLITE_OPEN_READONLY** 只读导出（口令是 YYNote 自带的 `<passphrase>`，不写库、不产生 -wal/-shm）；
4. `sync_yynote_todos.mjs` 幂等写入 `widget_state.json` 的 `department[]`，保留本地 `done`、不覆盖手工编辑。

没装小黄条 → 打印 `YYNOTE skip reason=no-db`，其它功能不受影响；装了但库被占用 → 关掉小黄条再跑一次即可。

### 17.5 验收四条（装完照这个确认）

```
pwsh -File <root>\doctor.ps1 # 期望：FAIL=0；WARN 只看 cargo/小黄条
node <root>\research\raw\emobile\em_login_state.js --auto --probe # 期望：OK … [PROBE] stream 建立成功
node <root>\research\raw\emobile\wxwork_sheet_pull.js --check # 期望：CHECK cookies=N ok=true
pwsh -File <root>\每日项目简报.ps1 -NoOpen # 期望：DONE … 告警 0；挂件右下角出现数据
```

`research\widget_state.json` 里 `department` 应有小黄条条目（`src:"yynote"`），`ui.tab` 默认 `proj`，切到「部门进度」即可看到导入的待办与彩色倒计时（逾期红 / 今天橙 / 未到期绿）。

## 十八、客户专属进度图（可选）：PDF + 私聊发送（2026-11-13）

> 用途：把「某一个客户」的项目从总看板里拎出来，做成 4 页图表 PDF，每个工作日 09:10 发给对接人。
> 本机实例 = 客户代号 `CLIENT_A`，本节的脚本名、规则文件都以 `client_` 开头；换客户时复制这四个文件改名即可。

### 18.1 客户名单怎么定（不要用总台账！）

`research/client_projects_extract.mjs` 的成员判定只认两条来源：

1. **文档品牌列 = 客户代号**：`research/wxdata/T1_summary_LIVE.txt`、`T2_summary_LIVE.txt`、`T2_summary_stop_LIVE.txt` 里 `R#### | 序 | 机型 | 图 | 品牌 …` 行的品牌单元格等于 `CLIENT_A`；
2. **群名锚定**：`research/raw/emobile/project_chatlog.json` 里群名含 `CLIENT_A / CLIENT_A / CLIENT_A / CLIENT_A / CLIENT_A / BRAND_X` 的会话，从中提机型。

**绝对不能**用 `research/brief_focus.json` 当客户名单：那是「负责人名下全部项目」，会混进其他客户（本机踩过：把 SELF_CO 自有品牌、CLIENT_B/CLIENT_C 客户的 6 个机型画进去，用户当场指出）。

**同族后缀变体**：同数字族即算同客户（`ITEM_X → ITEM_X/ITEM_X`、`ITEM_X → ITEM_X`）。用 `EXCLUDE_FAMS` 硬排除已知不属于该客户的族（本机：`ITEM_X`）。

### 18.2 一条命令出 PDF

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File .\research\client_progress_report.ps1 -DryRun # 只出 PDF，不发
pwsh -NoProfile -ExecutionPolicy Bypass -File .\research\client_progress_report.ps1 -Pull -Peer <uid> # 抓 出图 发私聊
```

参数：`-Pull`（先跑现成 `wxwork_sheet_pull.js` 抓 19 页签，不跑主抓取）、`-DryRun`（不建 WS、不发送）、`-ToName`（收件人显示名，默认「<收件人>」）。

产物：`CLIENT_A项目进度图_YYYY-MM-DD.pdf`（A4 横向 4 页：健康总览甜甜圈 / 项目甘特 / 风险矩阵 / 归档时间轴）+ `research/client_projects_daily.json`（逐项目 evidence）+ `research/client_send.log`。

### 18.3 图表口径（少文字）

- 绿 = 无逾期、无 P0 卡点、完成率正常；橙 = 7 天内到期 或 P1 卡点 或文档 × 群聊不一致；红 = 已逾期 / P0 卡点 / 归档节点已过但无实际；灰 = 文档没写归档日期（**显示「未排归档」，绝不编日期**）。
- 每行机型下面标来源（`台账` / `同族变体-CLIENT_A`），证据写进 daily.json 的 `evidence[]`，图上不放长文。

### 18.4 发给谁、怎么发

现役 `research/em_send.ps1 -Peer <uid>`：读 `research/raw/emobile/api/msgconn.json` 的本机客户端令牌 / ws_url / authcid，**短连接**（SCRAM 绑定 → 发消息 → 立刻 close）；收件人用 `<uid>` 参数，不写死姓名。旧发送脚本已删除。

硬约束：
- 绝不调用 `/emp/passport/login`、`/emp/api/msg/gettoken`：这两个会重建服务端消息会话，约 1 分钟后把用户桌面客户端顶下线；
- 不常驻 WS（服务端同 `udid` 只保留一个会话，常驻必顶号）；
- 首次务必 `-DryRun`，确认 PDF 和收件人后再去掉 `-DryRun`。

### 18.5 接进 09:10 定时

工作日 09:10 的会话定时 prompt 加一段（本机已加）：

```
先跑 pwsh -NoProfile -ExecutionPolicy Bypass -File <安装根>\research\client_progress_report.ps1 -Pull -Peer <uid>，
生成客户进度图 PDF 并私聊发送给对接人；简报里只报 PDF 路径 / 发送状态 / 红黄绿项目数。
```

不改 Windows 09:05 那个抓取任务，不新建第二个任务。

---

## 十九、发消息：把结果直接推到同事的 E-Mobile

> 目标：抓完数据后，把「文字 + 长图」发到指定同事的 E-Mobile 私聊，不需要他再点开什么系统。

### 19.1 一条命令

```powershell
# 只发文字
pwsh -NoProfile -File <root>\research\em_send.ps1 -<uid> -Text "CLIENT_A 项目进度已更新"

# 文字 + 图片（推荐：一组可读的摘要 + 一张长图）
pwsh -NoProfile -File <root>\research\em_send.ps1 -<uid> -Image <root>\research\client_report_long.png -Text "详情见下图"

# 先验证不动手
pwsh -NoProfile -File <root>\research\em_send.ps1 -<uid> -Text "test" -DryRun
```

`-Peer` 是对方的 uid（不是姓名）。uid 从会话快照 `research\unread_snapshot.json` / `conversations_latest.json` 历史兼容字段的 `targetid` 里取；现役以 `unread_snapshot.json` 为准
（群会话形如 `1266,<UID>`，私聊是单值或双人串，取不是自己的那一半）。

回执是 JSON：`{"sendOk":true,"peer":"169","mediaId":"...","shareOk":true,"thumbBytes":162122,"frames":2}`。

### 19.2 图片为什么要三步

| 步骤 | 接口 | 说明 |
|---|---|---|
| 上传 | `POST /emp/api/media/upload`（formdata `media` / `type=image` / `from`，头 `access_token`） | 拿到 `media_id`，这一步只是把文件放到服务端 |
| 共享 | `POST /emp/api/media/share`，body **必须是字符串** `{"media_ids":"<id>","touserids":"169"}` | **漏这步 = 对方收得到图但点开是空的**；传数组会 500 |
| 发送 | WS `<message type='chat'>` 发 `RC:ImgMsg`（`content` 是 JPEG base64 缩略图，`extra.imgUrl` 是 media_id） | 再补一条 `RC:TxtMsg` 发文字 |

缩略图上限约 250KB：脚本会自动用 Pillow 压到宽 1080、质量 ≤82。

### 19.3 前置条件与硬约束

- 本机 **E-Mobile 桌面客户端处于登录状态**（token 从它的进程内存里取，`em_access_from_memory.js`）。
- **绝不要调 `/emp/passport/login`**：会重建服务端消息会话，约 1 分钟后把在线客户端顶掉（顶号）。
- 不要常驻绑定 WS：发完就关（脚本 2.5–6 秒自动退出）。常驻绑定会永久占用 `<UID>|c` 这个 udid。
- 合成出来的消息**不进服务端历史库**（回读查不到自己的消息），只能靠收件人目视确认；要留痕就同时写本地日志。
- 只在私聊用；群发不做（同名/多端会有权限差异）。

### 19.4 接进日报（现成示例）

`research\client_progress_report.ps1 -Pull -Peer <uid>` 已经串好：
抓 19 页签 → 只保留在跑项目 → 出 PDF（留本地）+ 长图 → `em_send_brief.js --peer <uid> --image <长图> --text <摘要>`。
想换成别人/别的客户，复制这个脚本改 `-Peer` 和 `client_` 前缀的脚本即可。

### 19.5 排错表

| 现象 | 原因 | 处理 |
|---|---|---|
| `MODULE_NOT_FOUND: ws` | Node 找不到 `ws` | 套件已内置 `research\node_modules\ws`；`em_send.ps1` 会自动设 `NODE_PATH` |
| `sendOk:true` 但对方没收到 | 服务端没推、或对方不在会话里 | 让对方把会话点开一次再发；确认 `-Peer` 是对方 uid |
| 文字到了、图点开空白 | 漏了 `media/share`，或 `extra.imgUrl` 不是 media_id | 看回执 `shareOk` 必须为 true |
| `NO_IMAGE` | 传了 `-Image` 但路径不存在 | 用绝对路径；`-DryRun` 可先验证 |
| 客户端被挤下线 | 有人调了 `passport/login` 或常驻绑定了 WS | 让客户端重新登录；之后只用短连接 |

## 二十三、更新仓库前必须跑的门禁链

一次改动要从「重建清单」一路走到「CI 绿」，中间跳任何一步都会返工。顺序固定：

1. 重建套件清单：让 `manifest.json` 的 sha256 与字节数跟实际文件对上。
2. 重建导出仓：占位化 + 覆盖层 + 清单重建，确认占位化后无残留类别。
3. 跑门禁脚本：敏感词、必备文件、清单、安装顺序、exe 政策、**占位符完整性**（每个文件还原 BOM/换行后哈希必须等于清单；清单文件里不得出现盘符绝对路径；仓库内必须仍有 `<ROOT>` 占位符）。
4. 分离部署演练：克隆到 A 目录，安装到 B 目录，再在 B 跑体检脚本，必须输出「脚本与套件清单一致」。**不要只验同目录部署**：同目录会命中未改写的套件源，看不出安装器把谁改坏了。
5. 打包：从 HEAD 生成 zip，体积异常小说明 HEAD 是空树。
6. 推送 main 并建 Release，确认资产体积与本地一致。
7. 等 CI 结论为 success 再收工；红了就看日志修完从第 1 步重跑。

判据与失败处置见 `docs/30-更新说明.md` 的「发布前必跑」一节。核心原则：**发布是一条链，不是一个动作**。

## 二十四、会议安排数据链路（可选能力）

挂件第三个标签「会议安排」的数据来自本机已登录的会议客户端（泛微 Ecology），不是手工录入。

**依赖**

1. 会议 cookie 采集器（本机开发树专用，**不随公开套件分发**）：内部树里由 `em_cookie_scan.cs` 编译，`setup.ps1` 会自动尝试 csc/Add-Type）；
2. `research/ec_meeting_fetch.py`（随套件安装）；
3. 目标机器上会议客户端**已登录**（Cookie 只在其进程内存里，不落盘）。

**验证（三步，任一步失败就停）**

```powershell
# 1) 采集器在不在
Test-Path <ROOT>\research\em_cookie_scan.exe   # 仅内部完整树有；公开套件不含
# 2) 取数并落盘（退出码 0 = 成功；2=无客户端 3=无 Cookie 4=接口失败）
python <ROOT>\research\ec_meeting_fetch.py --out <ROOT>\meeting_data.json
# 3) 看产物
Get-Content <ROOT>\meeting_data.json -TotalCount 20
```

**挂件侧**：exe 每 10 分钟自动跑一次上面的第 2 步（`CREATE_NO_WINDOW`，不弹窗），成功即热载；会议开始前 5 分钟弹置顶提醒，点「知道了」才关。

**排错**

| 现象 | 原因 | 处理 |
|---|---|---|
| `meeting_data.json` 不更新 | 客户端未登录 / Cookie 采集器缺失 | 先登录客户端；重跑 `setup.ps1`，再手动执行验证第 2 步看退出码 |
| 弹窗不出现 | 会议开始时间已过 / 不在 5 分钟窗口 | 用第 2 步的产物确认 `today.items` 里有未来 5 分钟内的项 |
| 地点/参会人为空 | 接口只对「下一场」返回详情 | 已知边界，其余场次显示「—」 |

## 二十五、无脑部署（任意 Windows 电脑）

按顺序执行，每步都有「通过判据」，不通过就停在那一步排错。

1. 装前置：Node 18+、Python 3.10+、企业微信 PC 已登录。
   判据：`node -v`、`python -V` 有输出；企微窗口能看到会话列表。
2. 克隆仓库：`git clone <仓库地址> <REPO_DIR>`。
   判据：目录里有 `skills\project-board-kit\setup.ps1`。
3. 安装：`powershell -ExecutionPolicy Bypass -File skills\project-board-kit\setup.ps1 -Root <TARGET_DIR>`。
   判据：末尾打印 `已编译 企微只读密钥扫描器`、`已复制 32 位 libcrypto`。
4. 探测环境：`<TARGET_DIR>\discover.ps1`。
   判据：写出 `research\config.json`，`paths.python/node` 有值。
5. 体检：`<TARGET_DIR>\doctor.ps1`。
   判据：`[OK] 脚本与套件清单一致`；环境类 FAIL（服务器地址未填、客户端未登录）属正常。
6. 首次抓取：`<TARGET_DIR>\每日项目简报.ps1 -PullOnly`。
   判据：`DONE ... 页签 N · 群 N`。
7. 客户端本地能力（全部只读，不注入、不改内存、不调登录接口）：
   `research\wxwork_enable.ps1 -CheckOnly` → `research\wxwork_todo_local.py --days 180`
   → `research\wxwork_unread_build.py --hours 24`。
   判据：`TODO_LOCAL ok items=N`、`WXU plain=True chat=True rows=N`。
8. 挂件：`tools\项目看板.exe --mode normal --state ... --data ... --log ...`。
   判据：日志 `[gui] shown ... mode=normal`、`[meet] loaded ...`。
9. 定时：把 `每日项目简报.ps1 -NoOpen` 加进计划任务（工作日 09:10）。

客户端数据能力矩阵（同机同账号即可用，换机需重新跑第 7 步取密钥）：

| 数据 | 来源库/接口 | 脚本 |
|---|---|---|
| 群聊/私聊消息 | `message.db` | `wxwork_chat_export.py` |
| 未读计数 | `session.db` 的 `unread_conversation_table` | `wxwork_unread_build.py` |
| 待办 | `forever_store.db` 的 `todo_v4` | `wxwork_todo_local.py` |
| 日程 | `calendar_r7.db` | `wxwork_calendar.py` |
| 在线文档 | 客户端 cookie | `wxdoc_pull_all.mjs` |

换机注意：库密钥来自进程内存扫描（签名定位，与版本无关），必须先由本机客户端登录过一次；
文档目录被 OneDrive 重定向时，脚本会依次尝试 `Documents`、`OneDrive\Documents`、`OneDrive\文档`。

---

## 二十六、发布前的准备（每次改完都要过一遍）

发布前必须同时满足下面八条，任何一条不过都不要发：

| # | 检查 | 判据 |
|---|---|---|
| 1 | 版本三处一致 | `CHANGELOG` 顶部 = `Cargo.toml` = `config.rs` 的 `APP_VERSION` |
| 2 | 挂件二进制随 Release | Release 里有 `project-board-widget.exe`，`--version` 等于本次版本 |
| 3 | 会议采集器（可选） | 内部完整树才有；公开套件不含，会议标签会降级为空 |
| 4 | 离线依赖随包 | `files/vendor/wheels/*.whl` 与 `files/vendor/libcrypto32.dll` 存在 |
| 5 | 清单可重建 | `scripts/rebuild_manifest.ps1 -Root <root>` 后 doctor 显示「清单一致」 |
| 6 | 占位符存活 | 清单里仍有含 `<ROOT>` 的模板；无盘符绝对路径 |
| 7 | 二进制不进文本扫描 | 门禁/导出扫描跳过 `.whl/.dll/.exe/.png`（随机字节会误判） |
| 8 | 空目录冷启动 | `git clone --depth 1` + `install.ps1` + `doctor.ps1` 全绿（除环境项） |

一条命令跑完：`python _business/gh_push_gate.py --version vX.Y.Z --message "feat: ..."`

发布后：上传预编译 exe 作为 Release 资产 → 记一条 CHANGELOG（改了什么/为什么/怎么验证）→ 空目录再验一次。
完整说明见仓库 `docs/50-发布前的准备工作.md`。

> 说明：内存扫描、cookie/key 读取、企微 DB 解密等取证工具（`em_memscan.cs`、`wxwork_key_readonly.cs`、`wxwork_db_decrypt.py`）**不随本套件分发**；套件只包含采集与出报所需的最小脚本。
