# 第三方组件与许可

本仓库自身代码以 MIT 许可发布（见 `LICENSE`）。下面这些第三方件随包或由安装脚本准备，
它们各自保留原有许可。

| 组件 | 位置 | 版本 | 许可 | 用途 |
|---|---|---|---|---|
| cryptography (Python) | `skills/project-board-kit/files/vendor/wheels/` | 50.0.2 | Apache-2.0 / BSD-3-Clause | 离线解密企业微信本地库 |
| cffi (Python) | 同上 | 2.1.1 | MIT | cryptography 依赖 |
| pycparser (Python) | 同上 | 3.12 | BSD-3-Clause | cffi 依赖 |
| OpenSSL 1.1 (libcrypto32) | `skills/project-board-kit/files/vendor/libcrypto32.dll` | 1.1.x (32 位) | OpenSSL License / SSLeay License | 只读密钥扫描器的 AES-128-CBC 校验 |
| Python embeddable | 安装脚本按需下载 | 3.12.x | PSF License | 解析脚本运行时 |
| MinGW-w64 / Rust 工具链 | 不随包，仅构建时使用 | - | 各自许可 | 从源码构建挂件 |

## 关于 libcrypto32.dll

该 DLL 取自本机已安装的企业微信客户端（`libcrypto-1_1.dll` 的 32 位版本），用于在只读扫描
进程内做 AES-128-CBC 校验。它不做任何网络或文件写入操作，只在内存里对候选密钥做单块解密验证。

如果你所在环境不允许分发 OpenSSL 二进制，可以删掉 `files/vendor/libcrypto32.dll`，安装脚本
会退化为「只编译扫描器」，运行时会提示缺少运行库；此时企业微信本地库解密不可用，
其它链路（E-Mobile 采集、微信文档、看板、挂件、日历）不受影响。

## 不随仓库分发的件

- 预编译挂件 `project-board-widget.exe`：随 GitHub Release 资产发布，版本与仓库 `Cargo.toml` 一致。
- 企业微信客户端、E-Mobile 客户端、任何客户数据：一律不随仓库分发。
