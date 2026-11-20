## v2.33.3 — 2026-11-20

# v2.33.3

这一版只做一件事：把「泄露」这件事从「靠人记得跑」变成「CI 每次必跑」，并修掉上一版发布时暴露的两个链路缺陷。不改数据链路、不改看板外观。

## 1. 通用秘密扫描进 CI

新增 `scripts/secret_scan.py`，纯标准库、不需要任何私有词表，因此可以直接挂在公开仓的 CI 上：

- 厂商令牌（`ghp_` / `github_pat_` / `AKIA` / `xox[baprs]-` / `AIza`）、私钥块、JWT；
- 本工程最敏感的一类：企微文档 cookie 名（`wedoc_sid` / `wedoc_skey` / `tdoc_uid` 等）带赋值、长 cookie 串、`Authorization: Basic/Bearer` 实体值；
- 内网地址（`10.x` / `192.168.x` / `172.16-31.x`）、`*.weaver.com.cn` 主机；
- 本机痕迹（`<ROOT>\<名>`、盘符专有目录）、邮箱、手机号。

和原有 E02 的分工：E02 用私有词表管「业务名 / 客户名 / SKU / 内网主机」，只在发布机上跑；`secret_scan.py` 管「凭据形态」，模式公开、CI 与本地都能跑。两者互补，CI 绿不等于 E02 通过，这一点已写进 `docs/20-检测治具.md`。

误报处理写死在脚本里：整行哈希与 `"sha256": "…"` 字段跳过，`127.0.0.1`、`localhost`、`<占位符>`、`example.com` 放行。手机号规则两侧不允许贴字母数字，否则 sha256 里一段 11 位数字就会误报（这条是首跑真踩到的）。

## 2. 发布闸不再把「一条绿」当成「全绿」

`_business/gh_push_gate.py` 等 CI 时原来只取该 sha 的第一条 workflow run。上一版发布时 `guard-meta` 是红的、`build-widget` 是绿的，恰好第一条取到后者，于是打印了 `CI success`，人也被骗过去。

现在：取该 sha 上全部 run（`per_page=50`），先等全部 `completed`，再逐个检查 `conclusion`，任何一个非 success 就 `GATE FAIL: CI 有失败的 workflow：<name>=<conclusion>`；全绿才打印 `CI success on <sha>（workflow 名列表）`。等待上限从 600 s 提到 900 s。

## 3. CI workflow 的源头修正

上一版的 `guard-meta` 之所以红，是因为改了业务仓根的 `.github/workflows/guard.yml`，而导出时会被 `_business/repo_overlay/.github/workflows/guard.yml` 顶掉。远端跑的是没有 `PREPUB_ALLOW_NO_WORDS` 的旧版，E02 在无词表时按设计硬红。

现在两份文件已逐字节对齐（同一份内容两处放），CI 里：`guard_meta` → `secret_scan` → 单测 → `Publish gate (E01-E13)`（`PREPUB_ALLOW_NO_WORDS=1`，注释说明为什么）→ 文档必备文件检查。

## 4. 发布链上补一道同类检查

`gh_push_gate.py` 在 prepublish 之后新增一步：对导出仓跑 `scripts/secret_scan.py`，命中即阻断推送。这样即使有人绕过 CI（直接推分支、或在别的机器上发版），凭据扫描仍然生效。

## 怎么验证

```pwsh
# 导出仓必须干净；业务树本来就含内部路径，应当有命中（证明规则会亮）
python -X utf8 _export\repo\scripts\secret_scan.py --root _export\repo    # SECRET_SCAN_RESULT=PASS files=…
python -X utf8 _export\repo\scripts\secret_scan.py --root .               # SECRET_SCAN_RESULT=FAIL hits=…

python -X utf8 _business\selfcheck.py                                     # SELFCHECK_RESULT=PASS steps=6
python -X utf8 _business\gh_push_gate.py --version v2.33.3 --dry-run       # 末行 GATE PASS（dry-run）
```

## 已知未解

- GitHub 的 `secret_scanning_non_provider_patterns` 与 `secret_scanning_validity_checks` 属付费 Secret Protection，本账号不可用，API 会返回 200 但不生效，只能由 `secret_scan.py` + E02 补位。
- 业务仓仍无 remote，发布只能经 `gh_push_gate.py`；这是设计，不是漏配。
- 台账与表格的双向缺口（`ITEM_X` 台账有、表里无）等业务确认。

## v2.33.2 — 2026-11-20

# v2.33.2

上一版把看板做对了，但「数据对不对」还是靠人眼看。这一版把几处**会静默给出错误数据**的地方改成可判别、可报警，并把发布链加了一道硬闸。

## 1. 未读面板不再把「读不到」当成「没有」

- 缺企微库密钥（客户端未登录/换机）以前 `exit 0`，管道记 ok、面板空着。现在返回 3，并在 `wxwork_unread.json` 里带 `degraded` / `reason`。
- `wxwork_chat.json` 缺失以前返回 `[]`（等价「确实没有未读」），现在按契约返回「不可用」并回退时间窗。
- 挂件侧新读这两个字段：降级、或快照超过 5 分钟没更新时，未读面板顶部直接显示「未读快照降级 / 已过期」，不再让人对着 0 条猜。

## 2. 消息类型按实测显示（不再出现「富文本」这种糊弄标签）

- 文件消息（20/15/34/102）显示 `[文件] 名称.pptx (137 MB)`；图片（14）显示文件名（`资源 1.png` 只留 `1.png`）；微盘文件（565）把「3 MB」文本转成字节数；空体事件显示「系统消息」。
- 截断改为保扩展名（原来 `text[:52]` 会把 `(137 MB)` 切掉），放宽到 64 字。
- 新增合成 protobuf 回归测试 `tests/test_wx_types.py`（8 例），不需要真实企微数据也能跑。

## 3. 里程碑逻辑收成一处，并带单测

- 抽出 `research/lib/milestone_view.js`（日期归一 / 六档状态 / 排序），构建时内联进页面；`node research/lib/milestone_view.test.mjs` 覆盖 ISO、Excel 串号、`M/D`、跨年、空值、冲突文案，共 19 条断言。
- 假日期全部判空：`0000`（不再是 `1899-12-30`）、8 位文本日期、`2/30` 这种不存在的日期（V8 会把它滚成 03-02，现在反查月/日拦住）。

## 4. 每日管道：并发、版本、cookie

- 入口加全局互斥 `Global\FRD-PIPELINE`：挂件轮询与手工/定时运行撞车时，后来者直接退出（exit 9），不再两个进程同时写 `wxwork_chat.json`。
- `widget_data.json` 改原子写（tmp + rename），避免挂件读到半写文件变空白看板。
- 挂件版本比对从 cookie 分支里提出来（之前被错误嵌套，几乎从不执行），并新增「cookie 剩余天数」：<3 天提示、<1 天按故障；数值写进 `数据完整性.json` 的 `cookieDaysLeft`。
- 日志 2 MB 轮转（`unread_live.log` / `widget_pull.log` / `cookie_renew.log`）。

## 5. 发布链加硬闸

- `gh_push_gate.py` 新增 `--confirm-publish`：有 `--version` 时必须填同一版本号，无版本号必须填 `YES`；不填连预检都不跑（exit 2）。
- 中途失败会打印半状态报告（本地 HEAD、本地 tag、已删本地 tag），并清掉本地 tag，避免下次撞 422。
- 推送凭据从 `git -c http...extraheader=...`（进程命令行可见）挪到 `GIT_CONFIG_COUNT / KEY_0 / VALUE_0` 环境变量。

## 6. 首屏只留「今天就要动手」的，其余折叠

- 逾期未完成从 54 条平铺改成「最严重 8 条 + `<details>` 折叠其余 46 条（按项目汇总：几条、最久几天）」，P0 卡点回到首屏。
- 折叠块里的行仍是 `<button>` / `.mrow`，点开还是项目抽屉。

## 7. 「未来 14 天节点」只剩一个数字

- 之前 KPI 与柱顶数字是 30（明细行），标题与时间轴是 29（去重节点），同一屏两个口径。
- 现在统一为明细行口径（今天这批数据是 28），`_page_guard.py` 的注释写清「条 = 明细行」，去重节点不再对外展示；KPI 用占位串在建页时替换，静态 HTML 也不会再写死旧值。

## 8. 页面模板拆文件，构建期能校验

- `build_daily_brief.mjs` 里 420 行 CSS、250 行 JS 从模板字符串挪到 `research/page/board.css`、`research/page/board.js`：`node --check research/page/board.js` 可校验，改样式不用在 2000 行里找。
- 里程碑库以 `/*__MIL_LIB__*/` 占位符注入，构建时替换；占位符缺失或替换后长度没变会直接抛错，不再静默出坏页。

## 9. 导出只认白名单，敏感工具不进公开仓

- `research/_export_build.py` 加显式白名单（六个目录 + 根目录四个文件），根目录散落的 `wxwork_*.py/.ps1/.cs`、`_review_*` 一律不进公开仓；输出里带 `NOT_ALLOWED=n`。
- 内存扫描 / cookie 读取 / DB 解密类工具（`em_memscan.cs`、`wxwork_key_readonly.cs`、`wxwork_db_decrypt.py`、`em_cookie_scan.cs|.exe`、`refresh_wxwork_cookies.ps1`、`rust-widget/src/wxhandle.rs`）**既不进导出仓，也不再同步进套件**；`preflight.py` 的 `SYNC_SKIP_FILES` 挡住回灌，相关文档改成「仅内部树，公开套件不含」。
- `prepublish_gate.py` 的 E02（业务词表零命中）缺词表从「静默 PASS」改成 FAIL；`selfcheck` 自动带 `_business/leak_words.txt`。
- 顺带删掉根目录 18 个重复脚本/评审截图（`wxwork_*` 与 `research/` 同名版本重复，`_review_*` 是评审临时件）。

## 10. 首屏能点、样式收敛、门禁不再空转

- 首屏 `.mrow` 与折叠块聚合按钮补上事件委托：之前 h2 写着「点任意行下钻」，实际点了没反应。
- 现役看板补 `.mfold` 样式（`_v4_rebuild.py` 注入），不依赖浏览器默认 `<details>` 外观；`summary` 有可见焦点环。
- `_page_guard.py` 自愈前先备份（模板不含新功能，一次自愈会整页回退）；备份改落 `research/_bak/` 且不入库。
- 导出链：overlay 跳过 `__pycache__`，`E08` 把编译缓存判红，`selfcheck` 子进程禁写字节码；`E12` 认 `research/*.cs`。
- 白名单做了负向用例：根目录探针文件 `git add -N` 后导出 `NOT_ALLOWED=1` 且未进导出仓。

## 11. 终审复核后的收尾

- **selfcheck 不再弄脏导出仓**：`_business/selfcheck.py` 与 `repo_overlay/scripts/{guard_meta,prepublish_gate}.py` 都设了 `sys.dont_write_bytecode = True`。此前 S0 在主进程 `import guard_meta` 会写出 `scripts/__pycache__/*.pyc`，而 E08 判红、S4 又排在 S0 前面，形成「selfcheck 全绿、prepublish 必红」的假绿；`selfcheck` 末尾还会再扫一遍导出仓有没有残留字节码。
- **页面自愈备份不再入库**：`.gitignore` 加 `*.html.bak-*` 与 `research/_bak/`，7 个含真实客户名的 `项目数据看板.html.bak-*` 退出索引（文件保留在磁盘，便于回滚）。
- **E09 的「白名单二进制 0 个」不再是死代码**：去掉重复的 `rel not in BIN_ALLOW`，计数只统计真正被声明放行的二进制；E08 标题改成「无备份/缓存残留（扫文件系统）」，因为它并不看 git。
- **口径断言不再假绿**：`_page_guard.py` 的「标题 vs KPI」交叉断言改为先断言「两边都能取样」，任一正则失配直接报红。
- **未读参数不再撒谎**：`--hours` 标注为回退路径时间窗，主路径按 `--max-age-hours`（未设时用 `max(48, hours)`）；`unread_rows()` 的 docstring 改成描述真实行为。
- **重建链自洽**：`_v4_rebuild.py` 在 `--inplace` 后自动跑 `_kpi_pop.py`（此前单独重建会缺 KPI 弹层样式，靠 guard 自愈整页回退）。

## 12. 发布前检查补上「依赖完整性」；公开仓彻底清掉历史泄露

- **E13 导出仓 Python 依赖完整**：用 `ast` 解析每个 `.py` 的 import，本地模块必须能在树里解析（`try/except` 包裹的可选依赖除外）。
  这条不是凭空加的：库解密器（内部树独有，不随套件分发） 按「取证工具不随套件分发」剔除后，
  未读构建脚本 与 待办本地脚本 仍然在 import 它  
  E10 只查「文档引用的脚本存在」，查不到 import，结果是**装完直接 ImportError**。
- **缺解密器时优雅降级**：两处 import 改成 `try/except`，缺模块时打印 `reason=no-decryptor`、
  写 `degraded: true` 的空快照（面板显示降级而不是假装「没有未读」）。公开套件因此可用，只是未读通道停用。
- **修掉降级路径上的真崩溃**：`unread_rows()` 在失败分支返回 `None`，`None + _pending_rows()` 直接 `TypeError`；
  改为 `(unread_rows(...) or []) + ...`。这是公开仓才会踩到的路径，本地有解密器一直没暴露。
- **远端彻底脱敏**：本次推送用全新根提交（`--fresh-history`）+ force 覆盖 `main`，旧 tag 与旧 Release 全部删除。
  历史里的 `em_cookie_scan.cs`、`wxhandle.rs`、`wxwork_db_decrypt.py` 等取证/解密工具不再可从任何 commit 取出。

## 怎么验证

```
node research\lib\milestone_view.test.mjs                              # MILESTONE TEST OK（19 条断言）
python -X utf8 -m unittest discover -s tests -p "test_*.py"             # 7 例 OK
cd _export\repo; python -X utf8 -m unittest discover -s tests -p "test_*.py"   # 15 例 OK
python -X utf8 _business\selfcheck.py                                   # SELFCHECK_RESULT=PASS steps=6 failed=0
cd _export\repo; python -X utf8 scripts\prepublish_gate.py              # PREPUBLISH_RESULT=PASS checks=13
pwsh -File .\每日项目简报.ps1 -PullOnly -NoOpen -NoWidgetRestart         # EXIT=0，alerts 只剩业务侧待确认项
```

## 已知未解

- `表格出现未收录机型 ITEM_X`：流水线当前唯一告警，需要人工确认是新项目还是录入笔误（确认后补进台账即可消警）。
- `会议安排数据(当日/当周/当月)` 在 2026-11-19 18:10 那轮报过 `errorCode 002 登录信息超时`，cookie 续期后 11-20 09:56 那轮已恢复；若再复现，单独查 `research/ec_meeting_fetch.py` 的会话 cookie。
- V4 模板里还留着一份里程碑逻辑 fork（`research/_模板_v4.html`），正常管线不执行它，但 `inject_sheet_truth.py` 在产物缺失时会回退到该模板；后续要么让模板也调 `FRDMIL`，要么删掉这条回退。
- 公开套件默认**不含 cookie 读取器与库解密器**（内部树提供）：未读通道会显示降级，E-Mobile 群聊与看板不受影响；这是分发策略，不是故障。

## v2.33.1 — 2026-11-19

### 改了什么

1. 修复企微未读预览取错消息：原来只取「最新一条文本」，而企微的未读计数覆盖所有消息类型。实测「产品部」unread=2 的最新消息是当天的富文本，面板却显示 11-11 15:30 的「会议室走起」，看起来像在抓已读的老消息。现在预览取最新一条任意类型消息，非文本给类型占位（`[图片]` `[文件]` `[语音]` `[视频]` `[链接]` `[表情]` `[富文本]` `[聊天记录]`，其余 `[非文本消息]`），不伪造正文。
2. 新增陈旧未读过滤：客户端本地 `unread_count` 不随手机端已读回写，会留下几天前的「未读」。预览超过 48 小时（`--max-age-hours`，默认 48）的未读只计入日志 `stale=N`，不进面板。
3. 同步两处已漂移的套件副本（`research/client_projects_extract.mjs`、`research/verify_board.py`）并重建套件清单。

### 为什么改

- 未读面板把 8 天前的消息标成「未读」，一旦出现过一次，这条通道就没人敢信了：看到红点先怀疑数据，而不是去看消息。
- 旧逻辑用文本消息代表整个会话的活跃度，等于用「最后一句话」冒充「最后一件事」，非文本消息多的群必然错。

### 怎么验证

- `python research/wxwork_unread_build.py --hours 24 --json <临时文件>` 输出 `rows=2 stale=0`，其中「产品部」一行显示当天时间 + `[富文本]`，不再是 11-11 的正文。
- 逻辑探针：伪造「最新文本在 8 天前 + 当天有一条富文本」的会话，预览取当天富文本；伪造「9 天前的未读」，输出 `stale=1` 且不进面板。
- `python -X utf8 _business/selfcheck.py` → `SELFCHECK_RESULT=PASS steps=6 failed=0 issues=0`。

## v2.33.0 — 2026-11-18

### 改了什么

1. 修复 `research/wxdoc_autorenew.ps1`：Node 路径写死成已删除的运行时（`…\.dsh\dsh-runtimes\…\node.exe`），导致每半小时一次的 cookie 续期连续 12 次全败。改为 `config.paths.node → PATH → NODE_BIN → ProgramFiles(nodejs) → 字面 node` 解析链，全缺失时显式 `NODE_NOT_FOUND` 并以 1 退出，不再静默。
2. 修复 cookie 到期监控：`EarliestExpiry` 读的是只有手工带 `--cookies` 才会更新的快照，8 天没动，`cookie_expiry_log.csv` 一直报同一个过期日（假数据）。现在取数前先用同一份只读客户端库刷新快照（零网络、约 1 秒），失败静默回退到旧快照。
3. 新增死路径扫描（WARN 不阻断）：`scripts/selfcheck.py` 扫全树，命中 `dsh-runtimes/<runtime>/dependencies/(node|python)` 这类本机残留路径即列出「文件:行号 + 原文」。说明性文字通过白名单排除，不制造噪声。
4. 统一 Node 解析：`每日项目简报.ps1`、`research/client_progress_report.ps1`、`research/client_confirm_send.ps1` 的 Node 定位改成同一套「config → PATH → ProgramFiles → 字面 node」，不再指向不存在路径，也不再写死盘符。
5. 新增群聊分流：群聊与私聊区改成左右两列，E-Mobile 最近 20 条、企业微信最近 20 条各一列并带来源标签；去掉右栏重复的企业微信面板（同一批消息原来展示两次）。
6. 重构项目详情里的里程碑：日期归一（`YYYY-MM-DD`、`M/D`、表格串号 `46344(11/18)`、去括号串号都认；`M/D` 按与今天最近的一年归一，跨年不再错年），状态只认日期真相（绿=已写回、黄=延期写回并标「延 N 天」、红=逾期未写回并标「逾期 N 天」、橙=今天到期、空心=未到期或无计划日期），按计划日期升序，图例带各状态条数。
7. 表头并列「已写回 X / 总数（文档登记 Y / 总数）」：文档写「已完成」但实际完成日期为空的行走红灯并在状态旁标 `(!)`，悬停说明缺哪一列，不再用文档措辞伪造绿灯。
8. 修复里程碑圆点不亮：状态类挂在上，CSS 选择器却写在圆点上（`.dw-mi em.done`），且与旧版统计格 `.dw-ms/.dw-mi` 撞名。改用独立类名 `.dw-ml` / `.dw-mrow`，两套规则彻底分开。
9. 修复假逾期：日期为空被 `Number('')=0` 当成 1899-12-30，页面因此出现「逾期 46344 天」。空值与非法值先判空，逾期天数回到真实区间。
10. 内部：`kit_sync` 停用 C3 改写（与 guard 的逐字节副本校验互咬），`rust-widget/README.md` 的 `cd` 示例改成中性写法；发布推送改用 header 传凭据、`http.version=HTTP/1.1`，令牌不再出现在 remote URL 与 `.git/config`。

### 为什么改

- 两个 cookie 缺陷是静默失败：管道每天照常出数，续期与到期监控都在骗人，等到凭证真过期（12-02）才会集体失效，届时看板、挂件、汇报一起停摆。
- 里程碑灯色与排序错、逾期天数假，会让「谁逾期、谁完成」这个最基本判断失真，看板就失去用处。
- 死路径扫描把「本机运行时被删」这类隐患从隐性变显性，下次换机器或删运行时能当场看到，不必再靠人工翻日志。

### 怎么验证

- `python -X utf8 scripts/selfcheck.py` → `SELFCHECK_RESULT=PASS steps=6 failed=0 issues=0`。
- `pwsh -NoProfile -File 每日项目简报.ps1 -PullOnly -NoOpen -NoWidgetRestart -Quiet` → 退出码 0，`alerts=(none)`，19 步全 ok（cookie 步约 1.4 秒）。
- `research/wxdoc_autorenew.ps1` 单独跑退出码 0，`cookie_expiry_log.csv` 当日行 `ok` 且天数为真实剩余天数。
- 看板实测三个机型：ITEM_X 37 行（绿 20 / 黄 10 / 红 5 / 空心 2，表头 30 vs 文档 33）、ITEM_X 30 行（绿 4 / 红 4 / 橙 1 / 空心 21）、ITEM_X 33 行（绿 14 / 黄 7 / 红 6 / 橙 1 / 空心 5），排序按计划日期递增，圆点颜色与状态一致。

## v2.32.0 — 2026-11-18

### 改了什么
- 随仓库新增 `scripts/selfcheck.py` 与 `scripts/gen_manifest.py`：一条命令跑完「清单重建 + 脱敏门禁 + 发布前检查 + 二进制口径回归」，一次列全失败项。
- 文档补「仓库侧自检」用法（`AGENTS.md`、`docs/10`、`docs/30`、`SKILL.md`）。

### 为什么改
- 发布链的失败信息分散在多个门禁，逐条发现会反复返工；把自检能力随包给使用者，改完代码即可本地判定，不必等发布。

### 怎么验证
- `python scripts/selfcheck.py` 输出 `SELFCHECK_RESULT=PASS steps=4 failed=0`。
- 未提交树（git 索引为空）会自动跳过依赖 git 索引的检查项。

## v2.31.2 — 2026-11-18

### 改了什么
- 文档：发布前先跑一次本地自检，把所有门禁失败一次列全，禁止「改一处、发一次、看一条错误」的循环。

### 为什么改
- 连续多轮发布出现「每轮只暴露一个问题」的返工，根因是失败信息分散在 preflight/导出/guard/prepublish 四步。

### 怎么验证
- 本地自检五项全 PASS（preflight / 导出 / guard / prepublish / 二进制口径回归）。
- 本版发布链一次通过：guard PASS、prepublish PASS、分离部署 OK、CI success。

## v2.31.1 — 2026-11-18

### 改了什么
- `scripts/policy.py` 成为二进制/文本 hash 的唯一口径：`guard_meta.py` 与 `prepublish_gate.py` 不再各自实现 `norm_bytes`，统一 import policy（找不到时退回自带实现）。
- `_business/preflight.py` 新增 P09（本地二进制 MZ/PE 校验 + 清单 hash 口径一致性），把「导出后才发现的二进制损坏」提前到发布前 2 秒发现。

### 为什么改
- v2.29.x 出现「门禁时红时绿」：同一份 exe 在导出链被按文本重写（PE 头错位 1 字节），而 guard/prepublish 各自的口径不同，导致排查反复。

### 怎么验证
- `python _business/preflight.py` → `P09 二进制可加载/hash口径: OK`，`PREFLIGHT_RESULT=PASS`。
- 发布链 `validate_binaries` 打印「二进制校验 OK（PE 头 + wheel 完整性）」。

## v2.31.0 — 2026-11-18

### 改了什么
- 导出链：二进制（.exe/.dll/.whl）改为按字节复制，不再当文本读写；清单对二进制用原始字节计算 hash。此前 `em_cookie_scan.exe` 在导出时被 CRLF 归一写坏 1 字节（6144→6132），PE 头 `e_lfanew` 指向错误偏移，部署后无法启动。
- 发布链：打包后对 zip 内 `.exe/.dll` 做 PE 头校验（`MZ` + `e_lfanew` 指向 `PE\0\0`），`.whl` 做 zip 完整性校验，失败即阻断，杜绝「资产齐全但不可用」。
- 套件：随包携带最新 `files/research/emobile/em_unread_live.js`（1 分钟双源未读刷新），修复干净安装缺少该脚本、旧安装沿用陈旧副本把 `build_widget.mjs` 的双源 signals 覆盖回纯 E-Mobile 的问题。
- 套件：移除一次性清理脚本 `_cleanup_2026-11-18.ps1`（占位符未替换、对终端用户无用途）。

### 为什么改
- 上游 v2.29.1 部署反馈：随包 exe 无法启动（PE 错位 1 字节）、企微未读被每分钟刷新覆盖、发布门禁只验存在不验可用。

### 怎么验证
- `python scripts/prepublish_gate.py --version 2.31.0` → `PREPUBLISH_RESULT=PASS`。
- 发布链 `validate_binaries` 打印「二进制校验 OK（PE 头 + wheel 完整性）」。
- 安装冒烟：解 zip → setup → doctor 输出 `[OK] 脚本与套件清单一致`。
- 解压后 `em_cookie_scan.exe` 字节数与源一致（6144）、可被 Windows 加载。

## v2.30.1 - 2026-11-18

改了什么
- 修发布链末尾摘要里 `t0` 未定义导致的 `NameError`（E13 回读之后崩溃，资产其实已上传成功）。

为什么改
- v2.30.0 已完整发布（zip 2,065,576 B + widget 846,848 B + CI 绿 + 资产回读 OK），但摘要行没打印出来。

怎么验证
- `gh_push_gate.py --version v2.30.1` 结尾出现 5 行摘要与 `GATE PASS`。

## v2.30.0 — 2026-11-18

改了什么
- 发布前体检新增 P08：扫描 `*.md`/`*.ps1` 里引用的 `files/...`，必须真实存在且已 `git add`（URL、通配符自动跳过）。这条直接对应上游 P1-2 那类「CHANGELOG 说随包、包里没有」。
- 发布链新增 E13：Release 建好后用 API 回读资产清单，`project-board-kit-vX.zip` 与 `project-board-widget.exe` 必须都在，缺一即失败。
- 发布结尾改为 5 行摘要（版本/commit/资产/门禁/耗时），中间细节不再刷屏。

为什么改
- 前几轮「发布看起来成功、实际资产不全」只能靠人肉回读；失败日志刷屏也消耗注意力。

怎么验证
- `python _business/preflight.py` 输出 `P08 文档引用文件: 缺失 0 / 未跟踪 0` 且 `PREFLIGHT_RESULT=PASS fails=0`。
- `python _business/gh_push_gate.py --version v2.30.0` 结尾出现 `E13 回读 Release 资产` 与 5 行摘要，且 `资产回读 OK` 同时列出 zip 与 widget。
## v2.29.1 — 2026-11-18

改了什么
- `_business/bump_version.py` 现在同时更新 `Cargo.lock`（`rust-widget` / `wxread` 两个根包）里的版本号，真源与套件副本都改。
- 发布链步骤「9b 编译并上传 project-board-widget.exe」恢复可用（此前 `cargo build --locked` 因锁文件版本滞后而失败）。

为什么改
- v2.29.0 发布时门禁全绿、zip 与 Release 已生成，但挂件二次编译在 `--locked` 下失败：`cannot update the lock file because --locked was passed`。

怎么验证
- `cargo build --release --locked` 在两棵树上都能直接通过（不再要求先 `cargo update`）。
- `gh_push_gate.py --version v2.29.1` 全链绿，日志含「必需资产齐全 2/2」「widget asset=project-board-widget.exe」。
## v2.29.0 — 2026-11-18

改了什么
- 挂件未读面板接入企业微信：`research/build_widget.mjs` 新增 `wxSignals()`，读 `research/wxwork_unread.json`，与 E-Mobile 未读合并后取前 3 条；每条带 `src`（EM/企微）、`kind`（group/direct）、`conv`（群名），Rust 侧角标直接可用。
- 时间单位统一：企微 `ts` 是秒，`wxSignals()` 里 `ts < 1e12` 时乘 1000，避免被 48 小时窗口过滤掉。
- 发布链新增 zip 资产断言：`gh_push_gate.py` 打包后校验 `files/research/em_cookie_scan.exe` 与 `files/vendor/libcrypto32.dll` 必须在包内，缺一即失败（此前 CHANGELOG 宣称随包、实际只有 .cs）。
- `doctor.ps1` 的「挂件版本与源码一致」：改用 `cmd /c` 捕获 GUI 程序 stdout；取不到版本时降级为 WARN，不再误报 FAIL。
- `setup.ps1` 的 Release 下载地址不再写死仓库 owner：按 `FRD_GH_OWNER` → `git remote` → `<GITHUB_USER>` 顺序解析。
- 首启与自启统一 `--mode normal`（普通窗口、不置顶）。

为什么改
- 上游反馈：P1-1 未读没接企微、P1-2 zip 缺资产、P2-1 版本检查误报、P2-2 一键安装 404、P2-3 升级后启动模式未迁移。

怎么验证
- 合成 `research/wxwork_unread.json` 跑 `node research/build_widget.mjs` → `widget_data.json` 出现 `[企微/group] 群名 | 发言人 | 内容`。
- `doctor.ps1` 无 exe 版本输出时显示 WARN，不参与 FAIL 计数。
- `gh_push_gate.py --version v2.29.0` 全链绿，日志含「必需资产齐全 2/2」。
## v2.28.0 - 2026-11-18

改了什么

- 新增 `scripts/policy.py`：二进制白名单、忽略规则、hash 口径的唯一来源，导出链与门禁共用。
- 本机发布前检查改为加载仓库内同一份脚本，消除两份逻辑漂移。
- 发布链自动编译并上传 `project-board-widget.exe`，版本与 tag 不一致直接失败。
- CI 新增 windows 构建与 Rust 单测任务。

为什么改

- v2.26.0 连续六次失败、v2.27.1 运行库被误拦，根因是同类规则分散在多处。

怎么验证

- `preflight.py` 与 `prepublish_gate.py` 全绿；Release 同时包含 zip 与挂件二进制。

## v2.27.2 - 2026-11-18

改了什么

- 修正导出白名单：`files/vendor/libcrypto32.dll` 真正进入发布包（v2.27.1 里被二进制过滤误拦）。

为什么改

- v2.27.1 声称补了运行库，实际导出仍跳过 `.dll`，别人装完企微链路依旧不可用。

怎么验证

- 导出统计 `SKIP_BIN` 不含该 dll；全新 clone 里 `Test-Path skills/project-board-kit/files/vendor/libcrypto32.dll` 为 True。

## v2.27.1 - 2026-11-18

改了什么

- 随包补上 `files/vendor/libcrypto32.dll`（企业微信只读密钥扫描器的运行库）。
- 新增 `LICENSE`（MIT）与 `THIRD_PARTY.md`（第三方件与许可说明）。

为什么改

- 上游反馈：别人部署后挂件没有企业微信信息。链路复盘发现扫描器能编译但缺 32 位 libcrypto 运行库，密钥取不到，后续解密与未读全部为空。

怎么验证

- 全新目录安装后 `research/wxwork_enable.ps1 -CheckOnly` 能识别扫描器与运行库；Release 两个资产版本一致。

## v2.27.0 — 2026-11-18

改了什么

- 发布链最前面加 `_business/preflight.py`：源树卫生检查（套件副本漂移、多余文件、垃圾入库、版本一致、盘符路径、清单过期、二进制一致），可一键自愈。
- 套件不再夹带 `files/research/raw/` 与一次性脚本，减小体积也避免把只在本机存在的路径带出去。

为什么改

- v2.26.0 发布连续失败，全部是源树脏状态导致，问题在导出链才暴露，每轮代价高。

怎么验证

- `python _business/preflight.py` 输出 `PREFLIGHT_RESULT=PASS fails=0`；`gh_push_gate.py --dry-run` 全绿。

## v2.26.0 - 2026-11-18

改了什么
- 挂件二进制分发：套件不再夹带陈旧 exe；setup 三级兜底（内置 → 本机 cargo 构建 → 从 Release 下载 project-board-widget.exe），Release 附预编译二进制。
- 会议采集器 em_cookie_scan.exe 随套件提供预编译产物（源码仍在，setup 仍会尝试本机编译）。
- 离线依赖：套件内置 cryptography / cffi / pycparser 的 cp312 wheel 与 32 位 libcrypto32.dll，setup 自动解压到嵌入式 python 与 research 目录，升级安装同样生效。
- 新增 files/_MERGE_POLICY.md（可覆盖 vs 只能合并清单）与 scripts/rebuild_manifest.ps1（本机化后重建清单）。
- doctor 新增「挂件版本与源码一致」检查（exe --version vs Cargo.toml）。
- wxdoc_autorenew.ps1 去掉写死的 WXWork 路径，改为环境变量/运行进程/常见目录探测。

为什么改
- 上游反馈 v2.24.0 部署：桌面挂件仍是 v2.0.0（缺会议安排页签）、会议采集器缺失导致会议链路降级、嵌入式 python 缺 cryptography、libcrypto 未落盘、清单无法重建。

怎么验证
- setup.ps1 / doctor.ps1 / rebuild_manifest.ps1 均通过 PowerShell 解析（0 错误），setup 保留 UTF-8 BOM。
- prepublish_gate 允许 vendor 下 dll 与 wheels（白名单），其余二进制仍拦截。
- 新机冷启动：无 cargo 时从 Release 下载 exe；有旧 exe 时 doctor 报版本不一致 FAIL。

## v2.25.0 - 2026-11-18

改了什么
- setup.ps1 的挂件部署改成三级兜底：套件内置 exe → 本机 cargo 从源码构建 → 从 GitHub Release 下载 project-board-widget.exe；三条都不通时给出明确指引，不再静默跳过。
- Release 起同时提供预编译挂件二进制（project-board-widget.exe），新机器不装 Rust 也能有桌面挂件。
- 开机自启参数由 --mode float 改为 --mode normal（float 会回落 state 里的模式，导致多机行为不一致）。

为什么改
- 仓库按设计只分发源码，套件内又不再带 exe，新机器 setup 后拿不到挂件，只能沿用旧版本 exe；用户实测「按仓库部署后桌面挂件还是老版本」。

怎么验证
- setup.ps1 通过 PowerShell 解析（0 错误）、保留 UTF-8 BOM。
- 预编译 exe 由导出仓（已占位化的源码）构建，--version 输出本项目版本号；二进制内不含业务名、客户名、内网地址。
- 冷启动：目录为空时 setup 走第 3 级下载，落盘 tools\项目看板.exe 后打印版本号。

## v2.24.1 - 2026-11-18

改了什么
- 看板「05 群聊与私聊」在群聊流右侧新增独立的「企业微信」面板：列出最近 10 条企微消息，行尾带群名，标明群聊/私聊会话数。
- 挂件未读面板与企微未读导出：时间标签区分日期，今天只显示 HH:MM，非今天显示 MM-DD HH:MM。

为什么改
- 群聊流按时间统一排序，企微消息时间较早会被 26 条上限挤出，用户看不到企微内容；此外旧消息只显示时刻（如 15:30）容易被误读为今天。

怎么验证
- 重建后该面板副标「群聊 10 个 · 私聊 0 个 · 显示最近 10 条」，前 3 行群名均为「SELF_CO&精益精开模打样群」，发送人与正文可读。
- 未读数导出同样本：今天消息显示 14:35；历史消息显示 11-11 15:30。

## v2.24.0 - 2026-11-18

改了什么
- 看板「05 群聊与私聊」改为双源：E-Mobile 项目群 + 企业微信（群聊与私聊），每行带来源角标（企微 / EM），按时间统一排序。
- 该段总数、群数、KPI 全部按合并后的口径统计。

为什么改
- 此前看板只吃 E-Mobile 的 project_chatlog.json，企业微信侧只有挂件未读面板能看到，看板里完全没有企微内容。

怎么验证
- 重建后该段：近 7 天 535 条 / 28 群；群聊流前 8 行均带「企微」角标（如「OWNER_B：好的，有进展或结果及时反馈下」）。

## v2.23.12 - 2026-11-18

改了什么
- 维护手册的品牌归属说明不再出现客户编号，改为「客户 A / 客户 B」表述。
- 解密脚本的文档引用指向现行的只读扫描器（原引用的是已删除的旧工具）。
- 删除仓库里遗留的 0 字节源码文件，套件副本与真源重新对齐。

为什么改
- 审计报告列为 P2：客户编号残留、工具引用失效、0 字节文件被跟踪。

怎么验证
- guard_meta.py：PARITY 68/0、PLACEHOLDER 85 文件无缺失无泄漏、GUARD_RESULT=PASS。
- prepublish_gate.py：E01–E12 全过。

## v2.23.11 - 2026-11-18

改了什么
- 底部「数据 … 更新」时间戳收敛为单一实现 footer_stamp()，三张卡片各只画一次；同时补上会议卡空态分支缺失的那次绘制。
- 移除会议卡原有的第二处「数据 <会议时间>」文字。

为什么改
- 部门卡此前既由 unread_block() 画一次时间戳、又由卡片自己画一次，两次叠加在同一坐标，alpha 同像素混合两次 → 比项目卡亮。会议卡则相反：空态分支提前 return，时间戳整块缺失。

怎么验证
- 三个标签逐帧采样同一区域（x42–330, y498–522）：项目 p98=77 / top1%=79.5，部门 77 / 79.5，会议 77 / 79.3。

## v2.23.10 - 2026-11-18

改了什么
- 三个标签的文字统一为同一颜色与同一字重（此前选中项用 600 字重 + 琥珀色，未选中用 500 + 灰蓝），选中状态只保留 2px 琥珀下划线。
- 面板底色 alpha 235 → 246，卡片填充 alpha 70 → 100；部门卡底部时间戳 alpha 170 → 150（与项目卡、会议卡一致）。

为什么改
- 用户多轮反馈「标签字比别的亮」。量化后确认：选中标签字重 600 比未选中 500 更粗，笔画覆盖率更高，在 12px 字号下会被读成「更亮」；颜色本身也不同。

怎么验证
- 同一帧内采样三段标签（框 110×28）：项目进度 p98=174 / top1%=173.6，部门进度 174 / 173.6，会议安排 164 / 172.0（同色同字重，剩余差异来自字形笔画覆盖）。
- 底部同一行：左「数据…更新」77 / 79.5，右「上次抓取…下次」76 / 78.5，差 1；抓取进行中右侧按设计变琥珀。

## v2.23.9 - 2026-11-18

改了什么
- 挂件面板不透明度：面板底色 alpha 235 → 246，卡片填充 alpha 70 → 100，减少背后窗口串色造成的「同一行深浅不一」。
- 部门卡底部时间戳 alpha 170 → 150，与项目卡、会议卡的时间戳完全一致。

为什么改
- 用户三次反馈「标签/底部文字颜色不一样」。实测三段标签背景像素已相同（27,27,28），差异来自两处：卡片时间戳在不同卡片里用了不同 alpha；面板过透，背后的窗口透过逐像素 alpha 的层叠窗口显示出来，形成矩形深色块。

怎么验证
- 同一窗口内采样时间戳行（y 498–522，x 42–330）：项目卡 p99=83 / 顶部 1% 均值 77.7，部门卡 p99=83 / 77.6，差 0.1。
- 三段标签背景采样均为 (26,28,34)（此前 (27,27,28)，仅整体底色因面板 alpha 提升而位移）。

## v2.23.8 - 2026-11-18

改了什么
- 一次性分析产物不再入库：Firecrawl 登录页快照、双仓评估页、临时截图与中间 json 共 11 个文件从索引移除并写入 .gitignore。

为什么改
- 这些是抓取与评估过程的副产物，不属于产品，留着会让仓库变脏、干扰审计。

怎么验证
- 版本库内 research 目录不再包含 fc_auth、fc_signup、评估页与临时截图；发布链 GATE PASS。

## v2.23.7 - 2026-11-18

改了什么
- 标签栏选中态不再画胶囊底色，只保留琥珀文字与 2px 下划线；三个标签的背景完全一致。

为什么改
- 选中底（无论琥珀 30 还是中性白 22）都会让该段比同排更亮/更深，读起来像「叠了一层」。

怎么验证
- 采样像素：三个标签背景均为 (27,27,28)；选中项仅文字为琥珀色并带下划线。

## v2.23.6 - 2026-11-18

改了什么
- 卡片底部「数据 <时间> 更新」与右侧采集状态统一为同一颜色与透明度（111,118,134,150）；空态提示同步。

为什么改
- 两者原本 alpha 170 / 150 不一致，看着像两种颜色。

怎么验证
- 空闲时左「数据 … 更新」与右「上次抓取 … · 下次 …」像素同色；抓取进行中右侧按设计变琥珀色，结束后回落。

## v2.23.5 - 2026-11-18

改了什么
- 挂件标签栏选中态改用中性白底（alpha 22），文字与下划线保持琥珀色。

为什么改
- 原来选中底是琥珀色 alpha 30，与同排两个未选中标签色相不同，看着像「颜色不一样 / 叠了一层」。

怎么验证
- 采样像素：未选中 (25,27,32)，选中 (44,46,51)，三通道同向抬升、无偏色；截图对照见交付说明。

## v2.23.4 — 2026-11-18

改了什么
- 内部会话通报文档不再随仓库分发（保留本地留档），避免把内部过程记录带上公网。

为什么改
- 该文件属于跨会话交接记录，含内部路径与会话标识；发布前检查 E11 也因为它使用破折号而阻断。

怎么验证
- `git ls-files | grep 会话通报` 为空；`python scripts/prepublish_gate.py` 全项 PASS。

## v2.23.3 — 2026-11-18

改了什么
- 模块索引补齐企微链路与发布治具条目（`wxwork_calendar.py`、`wxwork_daily_refresh.ps1`、`prepublish_gate.py` 等 13 行），并修正表格结构。
- 使用说明书新增「二十、企微链路与管道步骤 5a4–5a7」：四步脚本、产物、失败表现与手动补救命令。
- 清理一次性脚本与日志的版本库跟踪（保留本地留档）。

为什么改
- 上一版索引把补充行追加到「边界」说明之后，破坏了表格；企微链路在文档里缺入口，运维不知道该跑哪一步。
- 一次性清理脚本属于过程产物，不应随仓库分发。

怎么验证
- `python scripts/prepublish_gate.py` 全项 PASS；`git ls-files | grep _cleanup` 为空。
- `docs/10-模块索引.md` 的表格连续且包含 13 行企微/治具条目；说明书中能搜到「5a4–5a7」。

## v2.23.2 — 2026-11-18

改了什么
- 企微未读改以客户端自己的未读计数表 `unread_conversation_table` 为准：该表可用时，未读数 0 就显示「没有未读消息」，不再回退到「近 24 小时有消息」的时间窗。

为什么改
- 读完消息后客户端已清未读，但旧逻辑在真实未读为空时回退到时间窗，把已读消息又列出来。

怎么验证
- 在客户端把消息读完，等一次刷新（默认 1 分钟），挂件「未读消息」区显示「没有未读消息」；再次收到新消息时按会话逐条列出。

## v2.23.1 — 2026-11-18

改了什么
- `doctor.ps1` 的企微待办检查改为指向只读导出脚本 待办本地脚本，提示语同步。

为什么改
- 上一版仍提示旧的「内存取证」路径，新机器按提示操作会走错脚本。

怎么验证
- 干净克隆 → `setup.ps1 -Root <dir>` → `doctor.ps1`，输出
  `[OK] 企微待办导出脚本 已就位（只读本地库，零输入事件）` 与 `[OK] 脚本与套件清单一致`。

## v2.23.0 — 2026-11-18

改了什么
- 待办改从客户端本地库的 `todo_v4` 表只读导出（待办本地脚本），不再依赖内存取证。
- 未读改为读取会话库的未读计数表，窗口 3h→24h；@/富文本消息补正文回退与噪声清洗。
- 新增「二十五、无脑部署」章节与「客户端数据能力矩阵」，补三条坑（纯数字标题被过滤、未读窗口过短、待办不在消息库）。

为什么改
- 原未读按「近 3 小时有正文」猜测，跨小时/重启即归零；待办此前只找到「创建了待办」事件、拿不到标题。

怎么验证
- `python research/wxwork_todo_local.py --days 180` → `TODO_LOCAL ok items=N`
- `python research/wxwork_unread_build.py --hours 24` → `WXU plain=True chat=True rows=N`
- 挂件「部门进度」出现对应待办；「未读消息」只显示真实未读。

## v2.22.1 — 2026-11-17

# v2.22.1 发布说明

## 改了什么
1. `docs/20-检测治具.md` 补齐 E11/E12 两项检查的说明，并新增「自检（变异测试）」小节：11 个变异体必须逐项变红（`mutations=11 blind=0`），任何注入违规却仍 PASS 的情况会被工具直接判定「该检查无效」。
2. `docs/10-模块索引.md` 补 `wxwork_todo_merge.py`（企微待办并入部门待办）与 `scripts/prepublish_gate.py`（发布前 12 项检查 + 11 项变异自检）两行，避免运维时找不到入口。

## 为什么改
- E12 是本轮新增的检查（导出 C# 必须真编译），文档没跟上会让后来的人不知道这道闸存在。
- 待办合并脚本此前只在管道里出现，模块索引缺行，排障时要翻 pipeline 才知道它负责什么。

## 怎么验证
- `python scripts/prepublish_gate.py --version 2.22.1` → `PREPUBLISH_RESULT=PASS checks=12 failed=0`
- 文档内搜索 `E12`、`自检（变异测试）`、`wxwork_todo_merge.py` 均能命中。
- 管道实跑：`每日项目简报.ps1 -NoOpen` → 10 步全 OK（`企微待办并入部门进度 OK`），部门待办保持 2 条真实项、无乱码条目回流。

## v2.22.0 — 2026-11-17

## 改了什么
1. 发布前门禁新增 E12：导出的 C# 采集器必须在发布前真实编译通过，防止「占位化改动代码字面量」这类只在别人机器上才炸的问题再次流出。
2. 修复 E05（必备文档/重复章节）的自检变异体：原来注入的重复标题可能与现有标题不同名，导致「检查无效」却看不出来；现在复制 README 首个真实标题两次。
3. 仓库内门禁补齐 E12 注册，并让自检临时目录唯一、清理时对只读文件做 chmod 兜底，避免残留目录导致 PermissionError。
4. 仓库内门禁的 WINDIR 兜底不再写字面盘符（改用 `%WINDIR%` / `%SystemRoot%`），消除 E03 盘符泄漏。
5. 企微待办取证/合并加双重防噪：UTF-16LE 被按 UTF-8 误读产生的 mojibake（如「摜癥灯屳慤慴灜」）不再进入部门待办；同时清掉此前误并入的 5 条噪声条目。

## 为什么改
- 冷启动复验在别人机器上暴露了「脱敏把 `new byte[CLIENT_B]` 改成 `new byte[CLIENT_B]`」导致 C# 编译失败；只靠文档承诺不可靠，必须让门禁自己编译一遍。
- 自检若存在「盲」变异（注入违规却仍然通过），等于门禁形同虚设，必须逐条断言变红。
- 企微内存取证在未登录/编码不匹配时会产出乱码，直接写进部门待办会污染用户数据。

## 怎么验证
- `python scripts/prepublish_gate.py --version <ver>` → `PREPUBLISH_RESULT=PASS checks=12 failed=0`
- `python scripts/prepublish_gate.py --selftest --version <ver>` → `SELFTEST_RESULT=PASS mutations=11 blind=0`
- `python scripts/guard_meta.py` → `GUARD_RESULT=PASS`
- 干净克隆 `setup.ps1 -Root <dir>` → `[OK] 脚本与套件清单一致`，且 `已编译` 采集器相关行出现。

## 已知边界
- 企微待办仍需客户端处于登录态才能被动取证；未登录时脚本输出 `NOT_LOGGED_IN` 并退出码 3。

## v2.21 — 2026-11-17

改了什么
- 代码文件不再做「客户端编号→占位符」的数字替换（会把 AES 密钥调度缓冲的长度常量改坏，导致克隆后 C# 编译失败）；文档仍保留该替换。
- `setup.ps1` 的 libcrypto 探测不再写死盘符：优先用运行中的 WXWork 进程路径，再回退 Program Files / Program Files (x86) / LOCALAPPDATA / `<系统盘>\WXWork`。
- 新增 doctor 三项企微体检（库密钥 / 日历数据 / 待办取证与数据），并修正检查表在企微段之前输出的顺序问题。

为什么改
- 冷启动演练发现两个真问题：①导出的 `wxwork_key_readonly.cs` 编译报 `CLIENT_B` 未定义；②导出的 `setup.ps1` 里 `<ROOT>` 被占位化成 `<ROOT>`，永远找不到 libcrypto。

怎么验证（从公开仓全新克隆 → 装到空目录）
- `git clone` → `setup.ps1 -Root <新目录> -NoAutostart`：输出「已编译 企微只读密钥扫描器」「已复制 32 位 libcrypto」。
- `doctor.ps1`：`[OK] 脚本与套件清单一致 (83 个文件)`，四行企微项全部出现。
- 导出仓内 `csc /target:exe wxwork_key_readonly.cs` 编译通过。

已知边界
- libcrypto 需要本机装过企业微信；未安装时 setup 只给提示，不影响其他功能。

## v2.20 — 2026-11-17

改了什么
- `doctor.ps1` 新增三项企微体检：库密钥、日历数据新鲜度（48h）、待办取证脚本与数据；并把检查表输出移到企微段之后，解决"第 9/9b 节结果不显示"的顺序问题。
- `docs/00-维护手册.md` 新增"每日管道的企微/会议步骤（5a4–5a7）"表：脚本、产出、失败时的表现。

为什么改
- 安装后要能一条命令看清企微链路是否通（密钥/日历/待办三处），避免只看到"进程在跑"却不知道数据是否可用。
- 步骤表让运维知道哪一步降级可以容忍、哪一步必须修。

怎么验证
- `pwsh -File skills/project-board-kit/doctor.ps1` 输出含四行企微项：库密钥 / 日历数据 / 待办取证脚本 / 待办数据。
- 未启用时全部为 WARN（可选能力），不影响安装判定；启用后转 OK。

已知边界
- 待办取证要求客户端处于登录态并实际拉取过一次列表；未登录时显示 WARN 而非 FAIL。

## v2.19 — 2026-11-17

改了什么
- `wxwork_todo_capture.py`：先判断登录态（无 WXWorkWeb 子进程即 `NOT_LOGGED_IN`，退出码 3），不再对未登录客户端做无意义扫描。
- 抽取结果增加 CJK 占比过滤（< 0.5 直接丢弃），避免把内存里的二进制噪声当成待办标题。

为什么改
- 未登录状态下扫描只会命中缓存里的乱码片段，输出不可读且容易误判为"已取到待办"。

怎么验证
- 未登录：`python research/wxwork_todo_capture.py --rounds 1` → `NOT_LOGGED_IN ...`，退出码 3。
- 登录后：预期输出 `TODO OK items=N` 并写 `research/wxwork_todo.json`；随后管道 5a7 自动并入部门进度。

已知边界
- 待办仍要求客户端已登录并至少拉取过一次列表；取证只读内存，不产生任何输入事件。

## v2.18 — 2026-11-17

改了什么
- `wxwork_calendar_merge.py`：日历为空时返回 0 并给出说明（原先返回 4，让管道把"无未来日程"误判为降级）。
- `wxwork_enable.ps1`：python 探测增加 `tools/python`、PATH 兜底，避免 `$env:FRD_PYTHON` 未设时报 `Test-Path` 空参数错误。
- 每日管道 5a6/5a7 实测转 OK，整体只保留"cookie 自动续期"这一条环境告警。

为什么改
- 空日历是正常状态（缓存里没有未来日程），不应该让每日体检变红，否则真正的失败会被噪声掩盖。

怎么验证
- `python research/wxwork_calendar_merge.py` → `CAL_EMPTY ...（正常）` 且退出码 0。
- `pwsh -File research/wxwork_enable.ps1 -CheckOnly` → `WXWORK_ENABLE_OK decrypted=3`。
- `pwsh -File 每日项目简报.ps1 -NoOpen` → 各步骤 OK，`!!` 只剩 cookie 续期一条（企微未登录导致，属环境项）。

已知边界
- 企微未登录时 cookie 续期与待办取证都无法进行；登录后无需改代码即可恢复。

## v2.17 — 2026-11-17

改了什么
- 每日管道新增 5a7：企微待办取证文件 → 并入部门进度（幂等、只增不删、先备份 state）。
- 新增版本升级工具 `_business/bump_version.py`，保证 Cargo.toml、config.rs、CHANGELOG 三处版本一致。

为什么改
- 待办此前只能靠界面查看；合并步骤先落地，等取证文件出现即可自动进看板，不需要再改代码。
- 手工改三处版本容易漏，工具化后发布链不会再因版本不一致被门禁拦下。

怎么验证
- `python research/wxwork_todo_merge.py --src <样例> --state <副本>` 首跑 `added=2`、二跑 `added=0`（幂等）。
- `python research/ec_meeting_fetch.py` 输出 `MEETING OK today=1 week=3 month=10`。
- 发布链十项门禁全 PASS，分离部署清单一致。

已知边界
- 企微待办仍需客户端登录并实际拉取一次列表，才会被内存取证脚本捕获。

## v2.16 — 2026-11-17

改了什么
- 会议标签新增企微日历来源：只读解密本地 `calendar_r7.db`，条目并入 `meeting_data.json`（标题前加〔日历〕）。
- 新增企微待办被动取证脚本（只读进程内存标记扫描，零输入事件）。
- 每日管道新增 5a6 步骤：企微日历 → 并入会议数据。
- 维护手册新增"企业微信操作红线"：禁止输入注入、禁止结束/重启客户端、禁止改内存；只允许文件副本、只读内存、已登录 cookie。

为什么改
- 日历是会议信息的第二来源，可与 Ecology 日历互相补足；待办原先只能靠界面查看，被动取证避免任何输入事件触发客户端风控。

怎么验证
- `python research/wxwork_calendar.py` 输出 `CAL OK events=N`。
- `python research/wxwork_calendar_merge.py` 输出 `MERGE calendar=N ...`。
- `python research/wxwork_todo_capture.py` 在客户端拉过待办后输出 `TODO OK items=N`。
- 预发布门禁 `scripts/prepublish_gate.py` 十项全 PASS。

已知边界
- 日历缓存仅在客户端同步后更新；未登录状态下三脚本都只能读到旧缓存。
- 待办为服务端数据，必须等客户端实际拉取一次才能在内存中被观察到。

## v2.15

- setup.ps1 新增 5h：编译只读密钥扫描器、复制 32 位 libcrypto
- doctor.ps1 新增企微可用性检查（Optional，不阻断安装）

## v2.14

- 企业微信群聊/私聊只读接入（复制库副本+离线解密，零中断）
- 只读密钥扫描器（不注入、不改内存）+ 一键启用脚本
- 每日管道新增「企微未读 + 三源交叉验证」两步
- 维护手册与模块索引补齐企业微信链路与边界

# CHANGELOG

格式与规则见 `docs/30-更新说明.md`。最新在最上面。

## v2.13.1 — 2026-11-17

改了什么
- README 按实际仓库重写：目录列全 9 项，自检命令换成 scripts/ 下真实存在的脚本，版本指向 CHANGELOG 顶部与 Cargo 一致。
- docs/10-模块索引.md 改为按仓库实际文件生成（此前索引里列了不随仓库分发的脚本）。
- 新增 E11 文档风格检查：不写 emoji、不用破折号替标点、不整行加粗当小标题。
- E10 放宽：research 下的 json/txt/log/html 等视为运行时产物，只要求文档引用的脚本存在。

为什么改
- README 与实际仓库对不上（版本号、目录、自检命令、预编译产物描述自相矛盾）。
- 文档里残留的模板腔痕迹（emoji、破折号、整行加粗）影响可读性。

怎么验证
- python scripts/guard_meta.py → GUARD RESULT=PASS
- python scripts/prepublish_gate.py --selftest → SELFTEST PASS mutations=10 blind=0
- python scripts/prepublish_gate.py → PREPUBLISH_RESULT=PASS checks=11 failed=0

## v2.13 — 2026-11-17

### 改了什么

- HTTP 连接复用（Keep-Alive）：`net.rs` 新增进程级连接池 `POOL`（key = `scheme://host:port`，容量 8，满则关最旧），`session`/`connect` 不再随请求创建销毁：高频请求省掉重复的 `WinHttpOpen` + TLS 握手。
- 提醒去重表带上过期：`REMINDED` 从 `Vec<String>` 改为 `Vec<(key, 触发秒)>`，每次检查先丢弃超过 600 秒的条目，长期运行不再无界增长（此前只有 200 条上限）。
- 状态锁中毒不再静默：`with_ctx` / `with_ctx_return` 改用 `lock_ctx()`：中毒时继续拿内部状态执行，并首次在 stderr 记一条 `[ctx] 状态锁曾中毒…`，不再出现「界面无反应且无日志」。
- 补 Rust 单测（21 条）：`net.rs`（URL 解析/默认端口/IPv6 字面量/非法输入）、`meet.rs`（会议条目解析与缺字段容错）、`wxhandle.rs`（缓冲倍增边界）。`cargo test --release` `21 passed; 0 failed`。
- 版本：`Cargo.toml` / `APP_VERSION` `2.13.0`。

### 为什么改

上一版把「静默失败」清了一遍，剩下的是「长期运行才会显形」的三类问题：连接不复用导致高频请求慢、去重表只增不减、锁中毒后整机静默。单测补的是最容易回归的纯函数（URL 解析、会议解析、缓冲边界）。

### 怎么验证

```
cargo test --release                              # 21 passed; 0 failed
python -m unittest discover -s tests -v           # Ran 7 tests ... OK
pwsh -File scripts/sync_kit.ps1 -CheckOnly        # KIT_ALREADY_IN_SYNC
python -X utf8 scripts/prepublish_gate.py --selftest   # SELFTEST_RESULT=PASS
```

## v2.12 — 2026-11-17

### 改了什么

- 修 3 个 P1 崩溃/静默失效点（`rust-widget/src/main.rs`）
 - 托盘初始化失败路径的 `lg_slot.unwrap()` 改为 `let Some(lg) = … else { return }`，不再 panic。
 - `--dump-hit` 未传时的 `cfg.dump_hit.as_ref().unwrap()` 改为 `let Some(base) = … else { 记日志后跳过 }`。
 - 会议数据刷新失败也置「已完成」标志 改为 `st.code() == Some(0)` 才置位，非零退出下一周期重试。
- 修 5 个 P2 静默截断/无界增长
 - `net.rs`：响应体超 64MB 改为返回 `Err`（不再把半截 body 交给调用方）；响应头缓冲按实际长度动态分配（上限 1MB）。
 - `wxhandle.rs`：`handle_path` 缓冲改倍增（102432768，不足返回 `None`）；`read_all` 超 512MB 返回 `None`。
 - `main.rs`：`REMINDED` 去重集合超过 200 条时裁剪最早的。
- 修 3 个 P3：`dyn_query` 补 `FreeLibrary`；`net.rs::split_url` 支持 IPv6 字面量（`[::1]:8080`）；`ec_meeting_fetch.py` 把写死的主机占位改为运行时必填（缺配置 `exit 2`，不再白等 25 秒超时）。
- 性能：`net.rs` 的读循环缓冲提到循环外复用，不再每轮分配 64KB。
- 新增 `tests/`（stdlib unittest，7 条）：解析归一、时间窗切片、Excel 序列号/月日转日期；`guard.yml` 在发布门禁前跑 `python -m unittest discover -s tests -v`。
- 新增 `scripts/sync_kit.ps1`：真源 `research/`、`rust-widget/` 套件副本一键同步，`-CheckOnly` 供 CI 用，同步后自动跑 `scripts/guard_meta.py`。解决「两套树互相漂移」的根因。
- 版本：`Cargo.toml` / `APP_VERSION` `2.12.0`。

### 为什么改

外部审计（2026-11-17）指出 2 处真实 panic 风险、会议刷新失败后不再重试、以及 4 处「静默截断」（调用方无法感知数据不完整）。这类问题不会在功能测试里暴露，但会在长跑或异常路径上让数据悄悄变错。

### 怎么验证

```
python -m unittest discover -s tests -v          # Ran 7 tests ... OK
pwsh -File scripts/sync_kit.ps1 -CheckOnly       # KIT_ALREADY_IN_SYNC
python -X utf8 scripts/guard_meta.py             # GUARD_RESULT=PASS
python -X utf8 scripts/prepublish_gate.py --selftest   # SELFTEST_RESULT=PASS mutations=10 blind=0
```

## v2.11 — 2026-11-17

改了什么
- 新增「发布前全面检查治具」`_business/prepublish_gate.py`：一条命令跑 10 项检查（E01–E10），失败即退出码 1，
 覆盖内建门禁、真实业务词、本机绝对路径、清单可还原、文档齐全与重复章节、版本一致、安装链可执行、垃圾残留、
 超大/未声明二进制、文档引用一致性。
- 真实业务词表移出仓库（`_business/leak_words.txt`），治具自身不再包含任何敏感词。
- `guard_meta.py` 的一致性比对从 3 组 27 文件扩到「整个 research 树 + rust-widget/README」：`PARITY checked=55 bad=0`。
- 修 3 处残留命名、清 2 个误提交的调试 txt，并补 `.gitignore`。
- 治具自带变异自检（`--selftest`）：复制干净树、逐项注入违规，断言对应检查确实变红，防止「治具没说谎但范围不全」。

为什么改
- 之前的教训是：问题总在推上云端之后才被发现（清单字节数、副本滞后、敏感词表自曝、安装顺序自相矛盾）。
- 治具本身也可能有盲区，所以治具必须能被自检。

怎么验证
- `python -X utf8 _business/prepublish_gate.py --version v2.11` `PREPUBLISH_RESULT=PASS`
- `python -X utf8 _business/prepublish_gate.py --selftest` `SELFTEST_RESULT=PASS`
- 干净克隆后 `setup discover doctor`，doctor 输出 `[OK] 脚本与套件清单一致 (N 个文件)`。

## v2.10.1 — 2026-11-16

改了什么
- 移除 `rust-widget/.cargo/config.toml`（写死本机 MinGW 链接器，克隆后 `cargo build` 会 `os error 123`）；导出时同样剔除 `.cargo/`。
- 文档补一条：仓库不含 `.cargo/config.toml`，使用 GNU 工具链时把 MinGW `bin` 放进 `PATH`。

为什么改
- 换一台机器克隆后无法构建，是「安装即用」的硬阻断。

怎么验证
- 干净克隆内 `cargo build --release` 成功；`guard_meta.py` 的 `PARITY` 与 `PLACEHOLDER` 均 PASS。

## v2.10 — 2026-11-16

改了什么
- `skills/project-board-kit/files/rust-widget/` 与真源对齐（22 个 `.rs`，补 `meet.rs`/`net.rs`/`popup.rs`/`wxhandle.rs`），并补 `Cargo.toml`/`Cargo.lock` 的 Windows 特性依赖。
- `setup.ps1` 新增 `em_cookie_scan.cs` 编译步骤（csc 优先、`Add-Type` 兜底），否则会议数据链路永久失效。
- `doctor.ps1` 增加「会议 Cookie 采集器」「会议数据新鲜度」两项检查。
- `guard_meta.py` 新增 `check_kit_parity`（副本与真源逐字节比对）。

为什么改
- 套件副本落后于真源时，用户装出来的是旧行为；这类漂移此前完全看不见。

怎么验证
- `PARITY checked=27 bad=0`；套件副本内 `cargo build --release` 成功。

## v2.9.3 — 2026-11-16

改了什么
- 文档补「挂件 exe 不随仓库分发，需本机 `cargo build --release` 后复制」；doctor 的提示改为可执行的下一步。

为什么改
- 缺 exe 时原先只报「缺失」，用户不知道该怎么办。

怎么验证
- 干净目录跑 `doctor`，WARN 文案里直接给出构建与复制命令。

## v2.9 — 2026-11-16

改了什么
- 修真实业务残留：门禁禁用词改为拼接/转义生成；聊天记录构建改为按 `simplelist_` 前缀取最新；运行态验证脚本改用 `BOARD_URL` 环境变量；客户端路径改用 `$env:SystemDrive`。
- 门禁新增 `PATH_LEAK` 逐 token 判定（整行豁免会漏）。

为什么改
- 上一版把禁用词明文写进门禁脚本，等于用敏感词自查敏感词。

怎么验证
- 旧版本克隆反测报 3 条 FAIL，新版本 0 条。


## v2.8 — 2026-11-16

改了什么
- 挂件 exe 内置「会议数据自刷新」：每 10 分钟自动重新抓取会议数据，不再依赖外部计划任务。
 - 触发点：主循环计时器；间隔 600 s；首次启动立即跑一次。
 - 执行方式：子进程跑 `research/ec_meeting_fetch.py --out <data 同目录>/meeting_data.json --quiet`，隐藏窗口；完成后自动热载并重绘。
 - Python 解释器解析顺序：环境变量 `FRD_PYTHON` `<ROOT>/tools/python/python.exe` 系统 PATH 的 `python.exe`。
- 新增会议数据链路（可选模块，缺依赖时静默跳过，不影响其它功能）：
 - `research/ec_meeting_fetch.py`：读本机已登录客户端的会话 Cookie 拉会议窗口 归一出 `{at, today, week, month, all}`。
 - `research/em_cookie_scan.cs`：会话 Cookie 读取器源码（需本机有 C# 编译器自行编译）。
- 版本号 `APP_VERSION` `2.8.0`。

为什么改
- 之前刷新鲜度靠一个外部计划任务，装到别人机器上不会自动存在，卸载也容易漏；放进 exe 后「装了就有」。

怎么验证
1. `项目看板.exe --version` `项目看板 v2.8.0 (rich-text build)`
2. 启动后日志应出现 `[meet] refresh start` `[meet] refresh done code=0` `[meet] loaded at=…`
3. 等 10 分钟再看，应再次出现同三行（间隔 600 s）
4. `schtasks /Query /TN 会议数据刷新` 应报「找不到」：外部任务已移除

已知边界
- 会议「地点 / 参会人员」只有服务端接口能取，且只对「下一场」有效；更远与历史场次为空，界面显示 `—`。提前 5 分钟弹窗触发的正是「下一场」，因此弹窗一定有值。
## v2.7 — 2026-11-16

改了什么
- 新增「推送治具」约定：每次更新仓库必须按固定顺序跑一条门禁链，而不是手工分步执行
 1. 重建套件清单（`manifest.json` 的 sha256/字节数）
 2. 重建导出仓（占位化 + 覆盖层 + 清单重建）
 3. 门禁检查（敏感词/必备文件/清单/安装顺序/exe 政策/占位符完整性）
 4. 分离部署演练：克隆到 A、装到 B，`doctor` 必须输出「脚本与套件清单一致」
 5. 生成资产并推送 main
 6. 建 Release 并上传资产
 7. 等 CI 结论为 success 才算完成
- 文档补充了这条链的判据与常见失败处置

为什么改
- 前几版的返工都来自「少跑了一步」：忘重建清单、只验同目录部署、占位符被改写没人发现、推完不看 CI
- 把顺序固化成一条链后，任何一步失败都会立刻停住并指出是哪一步，不会带着缺陷发布

怎么验证
- 每次发布都从第 1 步跑到第 7 步，全部输出通过标记后才算完成
- 故意破坏任一步（例如改了套件文件不重建清单、或把 `<ROOT>` 写成绝对路径），链应在对应步骤失败

## v2.6 — 2026-11-16

改了什么
- `setup.ps1`：两处文件枚举（RepairPath 修复、替换占位符）改为共用单一 `$skipNames = @('config.json','secrets.json','doctor.ps1','discover.ps1')`，不再各写一份
- `guard_meta.py`：新增 `check_placeholder_integrity()`：清单内每个文件还原 BOM/CRLF 后 sha256 与字节数必须与 manifest 完全一致；清单文件里不得出现盘符绝对路径；仓库内必须仍存在 `<ROOT>` 占位符
- 文档示例里的示例路径改为 `<ROOT>` 占位（`yynote_export.ps1`）

为什么改
- 上一版只改了第一处排除名单，第二处仍会改写 `doctor.ps1` 自身的 `<ROOT>` 占位符，属同一 bug 的残留口子
- 清单哈希按占位符口径生成，任何外部批量改写（安全扫描、格式化工具、脚本）都会破坏完整性，需要在 CI 里有一道硬门禁兜住

怎么验证
- `python scripts/guard_meta.py` 期望输出 `PLACEHOLDER files=N missing=0 bad=0 leaked=0 has_ph=True` 且 `GUARD_RESULT=PASS`
- 分离部署（克隆到 A、装到 B）跑 `doctor.ps1` 期望 `[OK] 脚本与套件清单一致`
- 想把占位符改坏试试：改任一清单文件一个字节，或把 `<ROOT>` 写成绝对路径，门禁应立刻 FAIL

## v2.5 — 2026-11-16

改了什么
- 修复：`setup.ps1` 改写 `<ROOT>` 占位符时跳过 `doctor.ps1` / `discover.ps1`（这两个脚本只用 `$PSScriptRoot` 定位，不需要绝对路径）
- 修复：`doctor.ps1` 的占位符改为拼接生成（`'<' + 'ROOT>'`），并改用大小写不敏感替换，安装路径被合法改写后仍能正确还原再比对
- 清理：`setup.ps1` 里重复的一行 `<ROOT>` 替换、`doctor.ps1` 里重复的一次换行归一

为什么改
- 上一版的反向还原逻辑在「克隆到 A、安装到 B」的分离部署里失效：`setup.ps1` 会把 `doctor.ps1` 自身的 `'<ROOT>'` 字面量也替换成安装路径，导致还原变成「路径路径」的空操作，仍报 12 个文件「被改过」
- 这不是变量大小写问题（PowerShell 变量名不区分大小写），而是自指的占位符被外部改写

怎么验证
- 分离部署：克隆到 A `setup.ps1 -Root <B>` 在 B 跑 `doctor.ps1`，期望 `[OK] 脚本与套件清单一致 (N 个文件)`
- 同目录部署与 `python scripts/guard_meta.py` 仍应 `PASS`

## v2.4 — 2026-11-16

改了什么
- 修复：`doctor.ps1` 清单比对前把安装根目录反向还原为占位符，`setup.ps1` 合法改写路径后不再误报「被改过」（干净安装后清单为 `[OK] … 一致`）
- 修复：`setup.ps1` 收尾清单顺序改为 `discover.ps1 doctor.ps1 首跑`，与 `SKILL.md` 一致
- 修复：`SKILL.md` 安装顺序一句自相矛盾的表述统一为 `setup discover doctor 首跑`
- 新增：`scripts/guard_meta.py` 增加业务残留检查（客户代号、品牌、公司、人名、机型占位、凭据样例），命中即 CI 失败，防止回潮
- 修复：门禁自身在 Windows 路径下未被跳过（`scripts[\/]guard_meta\.py`），已纠正

为什么改
- 复验报告指出：安装后清单会把 setup 的合法改写算成「被改过」，且文档里存在两种互相矛盾的安装顺序
- 门禁只看通用敏感形状，业务词仍可能悄悄回到仓库

怎么验证
- 干净目录：`setup.ps1 -Root <dir> -NoAutostart` `discover.ps1` `doctor.ps1`，期望 `[OK] 脚本与套件清单一致 (N 个文件)`，其余 FAIL 仅环境类（服务器地址未填、客户端未登录）
- `python scripts/guard_meta.py` `GUARD_RESULT=PASS`

## v2.2 — 2026-11-16

改了什么
- 新增：`docs/20-检测治具.md`（三层治具方法与断言类别）、`docs/30-更新说明.md`、`docs/40-坑清单.md`
- 新增：AGENTS.md（给 Agent 的部署契约：顺序 + 每步判据 + 禁止事项）
- 新增：SKILL.md 第二十/二十一/二十二章（Agent 部署逐步、治具怎么建、更新说明怎么写）
- 调整：`docs/00-维护手册.md` 与 `docs/10-模块索引.md` 重写为方法版（只讲怎么做，不含具体值/路径）
- 移除：仓库中的业务数据、客户信息、人名、账号、机器路径与一次性脚本

为什么改
- 交付给他人维护时，文档里不能出现任何业务信息与私有路径
- 排查与验收依赖口头经验，导致同样的问题反复出现

影响面
- 数据层：无字段变更
- 页面/挂件：无可见变化
- 管道/定时：无步骤变更

验证
- 占位化门禁：`发布链脚本（仓库外）` `GATE_BAD_CATS=0`
- 文档自检：全仓检索具体值/路径/旧脚本引用 0 命中
- 冷启动：clone 到临时目录，按 `SKILL.md` 第二十章走一遍可跑通

兼容与回滚
- 无需重跑解析
- 回滚：`git revert <sha>`

## v2.3 — 2026-11-16

修复「按教程装不起来」的四类问题，并把它们变成门禁。

- 补齐采集脚本实体：`files/research/emobile/` 下现在真的有 `em_login_state.js`、`em_full_pull.js`、
 `build_chatlog.js`、`em_unread_live.js`、`em_access_from_memory.js`、`em_names_by_id.js`、
 `em_contacts_refresh.js`、`wxwork_sheet_pull.js`、`wxwork_handle.cs`、`em_memscan.cs`、`em_ws_auth.cjs`。
 清单里的 sha256 与实体一致，`doctor.ps1` 不再因解析失败而跳过比对。
- manifest.json 改为「实有清单」生成：bytes 是整数、sha256 是 64 位十六进制，且与磁盘逐项核对。
- 安装顺序纠正：`setup.ps1` `discover.ps1` `doctor.ps1` 首跑；`discover` 缺目录时提示「请先 setup」，
 不再直接 throw。
- setup 编译兜底：没有 csc 时用 `Add-Type` 编译 cookie/内存读取器；cs 源路径同时兼容
 `research/`、`research/emobile/`、`research/raw/emobile/` 三种历史布局。
- 看板 exe：不随仓库分发；`setup` 会提示从 Release 下载或自行 `cargo build --release`。
- 门禁升级（`scripts/guard_meta.py`，CI 每次跑）：
 1) 占位检查（客户/品牌/人名/内网地址/绝对路径/凭据形状）；
 2) 必备文件存在；
 3) manifest 可解析、bytes 为整数、sha256 合法、声明与实体双向一致、字节数相符；
 4) SKILL 安装顺序必须 setup discover doctor；
 5) 未分发 exe 时，文档必须交代获取或构建方式。

## v2.10 — 2026-11-17

改了什么

1. 交付包补齐：`skills/project-board-kit/files/rust-widget/` 与仓库真源同步（新增 `meet.rs / net.rs / popup.rs / wxhandle.rs`，更新 8 个既有模块与 `Cargo.toml`、`Cargo.lock`）。按文档自建挂件不再得到旧二进制。
2. `setup.ps1` 新增 5d：编译 `em_cookie_scan.cs`（csc 优先，Add-Type 兜底），会议链路的 Cookie 采集器随安装就绪。
3. `doctor.ps1` 新增两项可选检查：会议 Cookie 采集器是否存在、`meeting_data.json` 是否新鲜（180 分钟）。
4. 删掉 `main.rs` 里一条永远为假的本机工具目录候选路径（死代码）。
5. `ec_meeting_fetch.py`：docstring 改原始串消除转义告警；`BASE_URL` 给模块级默认值，import 不再可能 NameError。
6. `每日项目简报.ps1` 会议步骤编号 5c 5a4，与实际执行顺序一致（根目录与套件两份同步）。
7. `SKILL.md` 新增「二十四、会议安排数据链路」：依赖、三步验证、排错表。
8. 门禁新增 `check_kit_parity`：kit 副本与真源逐字节比对（rust-widget 全树 + 会议链路脚本），防止再次漏同步。
9. 导出剔除 `.cargo/config.toml`（内含本机链接器路径，会导致拿到仓库的人构建失败）；GNU 工具链改为依赖 PATH 中的 MinGW。

为什么改：第六次复核指出「新功能的 Rust 源码没同步进 kit 副本」：用户照文档自建出来的仍是旧版，等于功能没交付。同类漏同步必须由门禁拦住，不能靠人记得。

怎么验证

```bash
python _business/gen_manifest.py
python 发布链脚本（仓库外）
cd _export/repo && python scripts/guard_meta.py     # 需 PARITY checked=N bad=0 且 GUARD_RESULT=PASS
python _business/gh_push_gate.py --version v2.10 --message "..."
```

另：在 `skills/project-board-kit/files/rust-widget` 里 `cargo build --release` 必须成功（已验证产物 865,280 B）。

