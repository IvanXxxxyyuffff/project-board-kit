# project-board-kit

本地运行的项目看板工具链。从企业微信文档表格和 E-Mobile 会话里取数，生成一份本地网页看板和一个常驻桌面挂件，可以挂到计划任务里每天自动刷新。

仓库里只有代码、方法和模板。业务数据、人名、账号、凭证、机器路径都不在这里：这些在你自己的机器上运行时才产生，也只留在你的机器上。

## 这套东西能做什么

- 本地网页看板：把项目节点、里程碑、群聊里提到进度的消息汇总成一页 HTML。
- 桌面挂件：Rust 写的小窗，常显今日节点、部门待办、未读消息、会议安排，开会前 5 分钟弹提醒。
- 每日刷新：一条 PowerShell 管道串起取数、解析、生成、自检。

进度汇报推送（生成图片并发出消息）不在此仓库，需要的按自己的通道另写。

## 目录

```
docs/            方法文档：维护手册、模块索引、检测治具、更新说明、坑清单
research/        取数、解析、生成、自检脚本（Python / Node / PowerShell）
rust-widget/     桌面挂件源码；wxread 是读企业微信表格的子模块
scripts/         门禁与同步工具：guard_meta.py、prepublish_gate.py、sync_kit.ps1
tests/           纯函数单元测试
skills/          一键安装套件：setup / discover / doctor 与模板
.github/         CI：单元测试 + 门禁
AGENTS.md        Agent 接手时的十步流程与通过判据
CHANGELOG.md     每个版本改了什么、为什么改、怎么验证
```

逐文件职责见 [docs/10-模块索引.md](docs/10-模块索引.md)。

## 快速开始

前置条件：Windows 10/11、Node 18 以上、Python 3.10 以上、企业微信 PC 客户端已登录。

不需要 Rust 工具链：挂件只有源码在仓库里，要自己编译就装 cargo；不编译就用套件里的安装流程，跳过后三步即可，看板和管道照常工作。

1. 安装：按 [skills/project-board-kit/SKILL.md](skills/project-board-kit/SKILL.md) 里「部署到本机」一节逐步执行。顺序是 setup → discover → doctor，别打乱，原因写在同一节。
2. 首次抓数：跑一次 `每日项目简报.ps1 -NoOpen`，看 10 个步骤是否都是 ok。
3. 自检：见下节四条命令。
4. 想改造成自己的：读 [docs/20-检测治具.md](docs/20-检测治具.md) 学会给自己加断言，读 [docs/40-坑清单.md](docs/40-坑清单.md) 少踩已知的坑。

## 自检命令

```
python scripts/guard_meta.py                      # 占位值、清单一致性、安装顺序、副本同步
python scripts/prepublish_gate.py                 # 发布前十项检查，含文档与代码不矛盾
python -m unittest discover -s tests -v           # 纯函数单元测试
cargo test --release                              # 挂件单测（在 rust-widget/ 下跑）
```

四条都绿再推。出包后用一个全新 clone 复跑第一、二条，确认换台机器也成立。

## 三条硬约束

1. 只读凭据。从本机客户端已有登录态里取值，不调用会重建会话的登录接口，那会把在线端顶下线。
2. 不覆盖版面。生成器只写自己的产物文件，用户改过的看板与模板当只读。
3. 数据不入库。抓取结果、聊天记录、表格文本、状态文件一律不进版本库，`.gitignore` 已经拦住。

## 版本

当前版本见 [CHANGELOG.md](CHANGELOG.md) 顶部，和 `rust-widget/Cargo.toml` 的 `version` 保持一致。每次改动按 [docs/30-更新说明.md](docs/30-更新说明.md) 的格式写一条，写清改了什么、为什么改、怎么验证。
