<p align="center"><img src="assets/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

简体中文 | [English](README.en.md)

统一远程设备工作台 · **Demo V0.5.4**

本版优化远程桌面的刷新与输入响应，提供流畅、清晰和省资源模式；切页或最小化后暂停桌面传输，返回时恢复。默认保留设备原始分辨率。

在一个Windows客户端中连接Linux设备、操作远程桌面、传输文件、运行终端并查看电脑本地日志。

| 功能 | 当前能力 |
| --- | --- |
| 设备 | SSH密码/私钥认证，可选加密保存凭据，主机指纹确认、分阶段连接进度与失败定位 |
| 桌面与文件 | X11远程桌面与三种画质模式、离页暂停/返回恢复；双栏SFTP、盘符切换、拖拽传输、进度与取消 |
| 终端与日志 | 多终端、重启/关闭、断线恢复、命令框补全；本地按项目/日期记录与清理 |
| 赛台 | 可开关摄像头，8/15/20/30 FPS；设备性能与60秒曲线；全场左下角米制坐标和定位输入接口 |
| 设置 | 中英文、浅蓝/绿色/灰色主题；软件内更新（安装版与便携版） |

尚无定位生产程序，坐标默认未接入，不生成模拟实时位置。摄像头默认关闭；最小化、离页和退出回收采集，恢复后需手动开启。原生MJPEG减少树莓派采集重编码，但流量及部分电脑占用会提高，实际性能受设备和网络影响。

## 下载与使用

[下载最新版本](https://github.com/JvKan9Zz/PortCanvas/releases/latest) · [使用说明](docs/usage.md) · [定位数据接口](docs/pose-protocol.md)

提供Windows x64安装版与便携版，附SHA256SUMS.txt。客户端使用系统WebView2，电脑端运行无需Node.js、Rust或Python。安装模式沿用原配置；便携模式使用同目录data/。

## 源码

| 目录 | 用途 |
| --- | --- |
| [app](app) | Tauri 2 + Rust + WebView2工程与[构建说明](app/README.md) |
| [assets](assets) | 当前品牌与赛台原件 |
| [docs](docs) | 当前使用与定位接口说明 |

main只保留当前工程与必要说明。旧版本和历史研发资料见[Releases](https://github.com/JvKan9Zz/PortCanvas/releases)与[Git历史](https://github.com/JvKan9Zz/PortCanvas/commits/main/)；历史提交不改写。
