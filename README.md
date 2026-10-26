<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

简体中文 | [English](README.en.md)

统一远程设备工作台 · **Demo V0.5.7 Beta**

本版通过已连接的 SSH 按需检查并准备缺失的远端桌面、终端、文件与摄像头组件；日志可设置容量上限，自动清理最早的已结束记录。Windows 版继续提供软件内更新。

在一个桌面客户端中连接Linux设备、操作远程桌面、传输文件、运行终端并查看电脑本地日志。

| 功能 | 当前能力 |
| --- | --- |
| 设备 | SSH密码/私钥认证，可选加密保存凭据，主机指纹确认、分阶段连接进度与失败定位 |
| 桌面与文件 | X11远程桌面与三种画质模式、离页暂停/返回恢复；双栏SFTP、盘符切换、拖拽传输、远端删除、进度与取消 |
| 终端与日志 | 多终端、重启/关闭、断线恢复、可展开命令框与补全；本地按项目/日期记录与清理 |
| 赛台 | 可开关摄像头，8/15/20/30 FPS；设备性能与60秒曲线；全场左下角米制坐标和定位输入接口 |
| 设置 | 中英文、浅蓝/绿色/灰色主题；Windows 安装版软件内更新 |

尚无定位生产程序，坐标默认未接入，不生成模拟实时位置。摄像头默认关闭；最小化、离页和退出回收采集，恢复后需手动开启。原生MJPEG减少树莓派采集重编码，但流量及部分电脑占用会提高，实际性能受设备和网络影响。

Beta 首次补齐组件由电脑联网下载并缓存，再通过 SSH 传给树莓派，板端无须自行联网下载。界面分别显示下载、传输与板端准备状态；下载最多 32 路，不支持分段时使用单连接。自动下载目前适配 Ubuntu 20.04 ARM64 控制器，需保留可用的 APT 包索引、Python 3、dpkg 和已登录的 X11 桌面。已有组件与电脑缓存优先复用。

## 下载与使用

[Windows V0.5.7 Beta](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.7-beta.1) · [V0.5.7 Alpha（板端联网）](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.7) · [macOS V0.5.6](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.6) · [使用说明](docs/usage.md) · [定位数据接口](docs/pose-protocol.md)

Beta 是独立预发布，需手动下载安装；原 v0.5.7 作为 Alpha 保留。Beta 不会把同号 Alpha 提示为更新，未来更高版本仍走原签名更新流程。

Windows 提供 x64 安装器，手动安装可选择位置，软件内更新沿用已有目录和配置。Mac 提供面向 macOS 13+ 的 Universal DMG，包含 Intel 与 Apple 芯片代码，使用系统 WKWebView；目前仅适配 Demo V0.5.6，后续可能等正式版推出后再继续适配。两端运行均无需 Node.js、Rust 或 Python。

Mac 包采用 ad-hoc 签名，未经 Apple 公证，首次打开可能需要你按 macOS 提示在“隐私与安全性”中手动允许。Windows 使用 DPAPI，Mac 使用钥匙串保护保存的凭据；加密资料不支持直接跨系统解密。

不再发布便携版、说明压缩包、单独签名和校验清单。Release 除安装包及 GitHub 自动提供的源码外，仅保留 Windows 应用内更新所需的 latest.json，签名包含在其中。

## 源码

| 目录 | 用途 |
| --- | --- |
| [app](app) | Tauri 2 + Rust + WebView2工程与[构建说明](app/README.md) |
| [assets](assets) | 当前品牌与赛台原件 |
| [docs](docs) | 当前使用与定位接口说明 |

main只保留当前工程与必要说明。旧版本和历史研发资料见[Releases](https://github.com/JvKan9Zz/PortCanvas/releases)与[Git历史](https://github.com/JvKan9Zz/PortCanvas/commits/main/)；历史提交不改写。

## 许可证

PortCanvas 项目代码采用 [MIT 许可证](LICENSE)。第三方组件保留各自许可证，见 [第三方声明](app/THIRD-PARTY-NOTICES.txt)。

加密依赖与现有安全边界见[安全说明](docs/security.md)。
