<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

简体中文 | [English](README.en.md)

Windows 原生远程设备工作台。当前开发版本为 **v0.6.0-beta.1**，尚未发布；安装与实际使用仍可参考 [v0.5.95](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.95)。

原生工作台使用 Slint、Rust、原生 VT/RFB 和 Windows 系统媒体能力，保留 v0.5.95 的页面结构、图标与主要交互。

| 功能 | 能力 |
| --- | --- |
| 设备 | 启动时局域网 SSH 发现、密码/私钥、指纹核对、可选加密保存凭据、分阶段连接与取消 |
| 桌面 | X11 远程桌面、三档画质、适配/1:1、只读、剪贴板；在当前窗口内展开桌面并保留工具栏 |
| 文件 | 双栏 SFTP、盘符/目录导航、多选与拖拽、递归传输、覆盖确认、限速、取消与远端删除 |
| 终端与日志 | 多个持久 tmux 终端、中文输入法、复制粘贴、草稿/历史与补全；电脑本地日志及清理 |
| 赛台 | 按需相机预览、设备性能历史及定位输入接口；没有定位生产程序时显示未接入 |
| 设置 | 中英文、主题/外观、颜色/图片/视频背景及音量；独立玻璃质量与不透明度，默认 Full |

摄像头默认关闭，最小化、离页、断连和退出回收采集，恢复后手动开启。帧率与资源开销受设备、网络及所选媒体影响；卡顿时软件可建议降低玻璃质量，由用户决定。

## 预览与稳定版

原生 Beta 计划以 **PortCanvas Beta** 独立安装，数据位于 `%LOCALAPPDATA%/PortCanvas/Beta`，与稳定版分开。旧资料须经预览与确认后导入，源文件保留。Beta 更新使用单独的预览通道；当前尚无公开 Beta 安装包。

[Windows v0.5.95](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.95) · [macOS v0.5.95](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.95) · [使用说明](docs/usage.md) · [定位接口](docs/pose-protocol.md)

当前原生开发仅面向 Windows x64。Mac 继续使用已发布的 v0.5.95 Universal DMG，ad-hoc 签名、未经 Apple 公证，采用手动更新；后续适配按需另行安排。

## 源码与构建

| 目录 | 用途 |
| --- | --- |
| [app](app/README.md) | 当前 Rust/Slint 原生工程、构建及安装器 |
| [assets](assets) | 品牌与赛台原件 |
| [docs](docs) | 使用与接口说明 |

安装后的 Windows 原生应用不需要 Node.js、Rust 或 Python。Linux 设备功能按需使用已有 SSH、Python 3、tmux、SFTP、x11vnc、FFmpeg/OpenCV；自动准备目前面向 Ubuntu 20.04 ARM64 控制器。缺失组件可由电脑下载并通过 SSH 放入用户缓存，不替换系统 SDK 或已有组件。

历史实现与版本保留在 [Releases](https://github.com/JvKan9Zz/PortCanvas/releases) 和 Git 标签中，不重写历史。

## 许可证

项目代码采用 [MIT](LICENSE)。第三方组件、字体和 Slint 保留各自条款，见 [原生组件声明](app/native-probe/THIRD-PARTY-NOTICES.md) 与随安装器提供的通知。认证、更新及数据边界见 [安全说明](docs/security.md)。
