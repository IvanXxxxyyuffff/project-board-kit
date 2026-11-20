# AGENTS.md — 给「拿到这个仓库的 AI Agent」的部署与维护契约

> 你（Agent）要在一台新机器上把这个项目跑起来，并保证它能每天自己更新。
> 本文件只给方法、顺序、判据；所有具体值（主机、账号、盘符、ID）都必须在本机现场探测，不要假设、不要向我索要。
> 硬约束与坑见 `docs/40-坑清单.md`；每个模块做什么见 `docs/10-模块索引.md`；改动必须写 `CHANGELOG.md`。

---

## 0. 你的工作顺序（不要跳步）

```
探测环境 → 定位数据源 → 安装 → 首抓 → 解析 → 生成 → 自检 → 定时 → 交付说明
```

每一步都有「通过判据」。判据不过就停在那一步排查，不要往下走。

---

## 1. 探测环境（判据：两个版本号都能打印）

```powershell
node -v          # 需要 18+
python --version # 需要 3.10+（若没有，用套件自带便携版或先装）
```

- 目标：确认解释器可用，并记下它们的绝对路径（后面写进配置）。
- 客户端的登录态：确认目标机器上相关客户端已登录（看进程 + 数据目录存在即可，不要重启客户端）。

---

## 2. 定位数据源（判据：探测脚本写出配置，且路径 file-exists）

跑探测脚本（`skills/project-board-kit/discover.ps1`）。它会尝试找到：

| 数据源 | 找什么 | 找不到时 |
|---|---|---|
| 会话/消息客户端 | 进程 + 安装目录 + 本地数据目录 | 该通道标为不可用，不阻断其它通道 |
| 文档客户端 | 进程 + Cookies 数据库 + `Local State` | 同上 |
| 表格 DOC 标识 | 本地缓存/配置里的文档标识 | 让用户手工提供文档链接一次，或跳过 |
| 桌面便签待办（可选） | 便携数据库 + 原生库 | 打印 skip 原因，不阻断 |

原则：任何通道缺失都要能「降级运行」并明确打印原因，绝不静默变成 0 条。

---

## 3. 安装（判据：脚本退出码 0，`<ROOT>/tools` 下出现可执行文件）

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File <ROOT>\skills\project-board-kit\setup.ps1 -Root <ROOT>
```

- 演练/只需文件时加 `-NoAutostart`。
- 装完后必须检查：可执行文件存在、`research/` 下有默认配置、启动项是否按预期创建。

---

## 4. 首抓（判据：原始产物落盘，且条数 > 0）

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File <ROOT>\每日项目简报.ps1 -NoOpen
```

- 看日志每一步的 `ok/fail`，只允许「可选通道」失败（例如桌面便签未安装）。
- 失败时按 `docs/40-坑清单.md` 第 8/9/12 条先查环境与占用。

---

## 5. 解析（判据：中间 JSON 的实体数与原始行数量级一致）

- 表格解析要按表头匹配取列（不要按列号）。
- 同一实体在多个页签出现时，按「实体 + 日期 + 节点」去重。
- 解析完必须核对：实体数、有节点表的实体数、节点总数三个数字是否合理；异常先停下查，不要靠看板样式掩盖。

---

## 6. 生成（判据：页面与挂件数据同源，且自检通过）

- 生成器只写自己的产物文件；用户手工调过的版面文件只读。
- 样式/脚本注入用幂等 id（重复注入只保留一份）。
- 挂件启动必须显式传 `--state/--data/--log`，并统一用普通窗口模式（不要置顶）。

---

## 7. 自检（判据：三层全绿 + 一条命令全绿）

先跑仓库自带的一键自检（它把清单、脱敏门禁、发布前检查、二进制口径一次跑完）：

```
python scripts/selfcheck.py
```

输出 `SELFCHECK_RESULT=PASS` 再进行下面三层业务治具：

```powershell
python <ROOT>\research\verify_board.py     # L1 数据层
node   <ROOT>\research\verify_runtime.mjs   # L2 运行态
python <ROOT>\research\_page_guard.py       # 产物结构
```

- 有红：先修再复跑；不要把断言改成恒真。
- 治具本身报环境缺失（浏览器/端口）也要先修环境，不能当绿。

---

## 8. 定时（判据：手动触发一次后产物时间戳更新）

- 用交互登录上下文注册任务，否则「从未运行」。
- 任务命令里写解释器绝对路径，并把 Node/Python 路径同时写进配置（双保险）。
- 触发后检查：产物 mtime、日志最新时间、挂件仍在跑且只有一个实例。

---

## 9. 交付说明（判据：使用者能自己复跑）

把这几件事写进交付说明（不要写具体值，写「在哪里看」）：

1. 数据从哪来、多久刷新一次、看哪里确认刷新成功。
2. 三条只读/不覆盖/不入库的红线。
3. 出问题时先跑哪三条命令。
4. 想改口径时改哪个模块（对照 `docs/10-模块索引.md`）。

---

## 10. 绝对不要做的事

1. 不要调用任何「登录/换 token」类接口（会踢掉用户在线的客户端）。
2. 不要覆盖用户版面文件，不要重建用户数据文件（只做增量或备份后替换）。
3. 不要把数据、日志、凭据提交到版本库。
4. 不要在浏览器 vendor 目录里跑宽泛正则检索（会刷出压缩代码）。
5. 不要在命令行里拼含反斜杠/引号的长字符串（写脚本落盘再执行）。

### 挂件二进制需要本机自建（不随仓库分发）

> 工具链说明：仓库不含 `.cargo/config.toml`（它通常写死本机链接器路径）。用 GNU 工具链时请把 MinGW 的 `bin` 目录加入 `PATH`；用 MSVC 工具链则无需额外配置。

仓库里只有 Rust 源码（`rust-widget/`），没有编译好的 `项目看板.exe`。需要桌面挂件时：

1. 装 Rust 工具链（stable，MSVC 或 GNU 均可）；
2. `cd rust-widget && cargo build --release`；
3. 把 `target/release/rust-widget.exe` 复制成 `<ROOT>/tools/项目看板.exe`；
4. 用显式参数启动（缺参数会读错目录）：
 `项目看板.exe --mode normal --state <ROOT>\research\widget_state.json --data <ROOT>\research\widget_data.json --log <ROOT>\research\rust-widget.log`。

不需要挂件也能跑数据管线：`每日项目简报.ps1` 会在缺少 exe 时跳过挂件步骤并继续。

## 修改代码的硬规矩

1. 改一处，同步两处：`research/` 与 `rust-widget/` 是唯一真源，`skills/project-board-kit/files/` 下是同名副本。
 改完必须同步副本，否则 `guard_meta.py` 的 PARITY 会报「副本滞后」。
2. 版本三处一致：`CHANGELOG.md` 顶部、`rust-widget/Cargo.toml` 的 `version`、发布参数（`--version vX.Y`）。
 E06 会拦；`2.11` 与 `2.11.0` 视为同一版本。
3. 发布只走一条链：`python -X utf8 scripts/release_check.ps1 -Version vX.Y -SelfTest`，
 或依次跑 `guard_meta.py` `prepublish_gate.py [--selftest]`。两项都绿才算能发。
4. 生成的产物只能由生成器重建：页面/清单/导出树不要手工改，改了必然与门禁对不上。
5. 不要提交数据：凭据、cookie、聊天记录、表格原文、业务词表都留在仓库外。
