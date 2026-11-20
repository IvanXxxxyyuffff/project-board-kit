## 这个 PR 改了什么

<!-- 一句话说清动机，再列改动点。 -->

## 发布前自检（勾选）

- [ ] `python -X utf8 scripts/guard_meta.py` 通过
- [ ] `python -X utf8 scripts/prepublish_gate.py --version vX.Y` 通过
- [ ] 改了 `research/` 或 `rust-widget/` 后，已同步 `skills/project-board-kit/files/` 下的副本
- [ ] 版本号三处一致：`CHANGELOG.md` 顶部、`rust-widget/Cargo.toml`、发布参数
- [ ] 文档引用的脚本都真实存在（E10 会查）
- [ ] 没有把日志/备份/演练目录/本机路径带进来

## 影响面

<!-- 影响哪些脚本、哪些看板区块、需要对方做什么。 -->
