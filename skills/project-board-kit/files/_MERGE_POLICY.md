# 升级合并策略（重要）

本套件里同时存在两类文件，**升级时必须区别对待**：

## A. 可覆盖（实体文件）

纯代码/文档，与业务数据无关，可直接用新版覆盖：

- `rust-widget/src/**.rs`、`Cargo.toml`、`Cargo.lock`
- `research/*.py`、`research/*.mjs`、`research/*.js`、`research/*.ps1`（不含配置）
- `docs/**`、`scripts/**`、`tests/**`
- `每日项目简报.ps1`、`setup.ps1`、`discover.ps1`、`doctor.ps1`

## B. 只能合并（模板/本机化文件）

含本机真实值，**覆盖会丢失配置**。升级时逐字段合并，保留本机值：

- `research/config.json`、`research/secrets.json`
- `research/brief_focus.json`（本机项目台账）
- `research/widget_state.json`（待办/归档/窗口位置）
- `research/wxwork_todo.json`、`research/meeting_data.json`（运行时数据）
- `skills/project-board-kit/files/**` 下任何含占位符（`<ROOT>`、`<CLIENT_*>`）的文件：这些是**模板源**，不要直接拷进运行目录

## 升级步骤（推荐）

1. 备份运行目录（`backup/upgrade-<version>-<time>/`）。
2. 停止挂件：`Stop-Process -Name 项目看板 -Force`。
3. 只覆盖 A 类文件；B 类文件逐个比对后合并。
4. 重建清单：`pwsh -File scripts\rebuild_manifest.ps1 -Root <root>`。
5. `pwsh -File doctor.ps1` 必须 `FAIL=0`。

## 判定脚本化

`setup.ps1 -Merge` 会跳过 B 类路径（见脚本内 `$MergeSkip`），只覆盖 A 类；不带 `-Merge` 时为全新安装。
