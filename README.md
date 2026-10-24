<p align="center"><img src="04-数据与素材/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo"></p>

# PortCanvas

统一远程设备工作台 · **Demo V0.5.2**

在一个Windows客户端中连接Linux设备、操作远程桌面、传输文件、运行终端并查看电脑本地日志。

| 功能 | 当前能力 |
| --- | --- |
| 设备 | SSH密码/私钥认证，可选加密保存凭据，主机指纹确认 |
| 桌面与文件 | X11远程桌面、双栏SFTP、盘符切换、拖拽传输、进度与取消 |
| 终端与日志 | 多终端、重启/关闭、断线恢复、命令框补全；本地按项目/日期记录与清理 |
| 赛台 | 可开关摄像头，8/15/20/30 FPS；设备性能与60秒曲线；全场左下角米制坐标和定位输入接口 |
| 设置 | 中英文、浅蓝/绿色/灰色主题、版本检查 |

尚无定位生产程序，坐标默认未接入，不生成模拟实时位置。摄像头默认关闭；最小化、离页和退出回收采集，恢复后需手动开启。原生MJPEG提高预览速率、降低树莓派采集CPU，但流量与部分电脑占用上升，实际性能受设备和网络影响。

## 下载与使用

[下载 Demo V0.5.2](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.2) · [使用说明](docs/使用说明.md) · [定位数据接口](docs/定位数据接口.md)

发布包提供Windows x64安装版与便携版，附SHA256SUMS.txt。客户端使用系统WebView2，电脑端运行无需Node.js、Rust或Python。安装模式沿用原配置；便携模式使用同目录data/。个人配置、密钥、日志和相机照片不进入仓库或发布包。

## 源码

当前工程：[studio-tauri](03-实现与脚本/studio-tauri)。Tauri 2 + Rust + WebView2；[构建说明](03-实现与脚本/studio-tauri/README.md)。必要品牌与赛台原件在04-数据与素材。

本版通过44项核心、14项SSH集成、9项生命周期及安装/便携检查；没有实车定位精度结论。

main只保留当前版本工程和必要说明。旧程序、历史需求、设计过程和研发记录从[版本发布](https://github.com/JvKan9Zz/PortCanvas/releases)与[Git历史](https://github.com/JvKan9Zz/PortCanvas/commits/main/)获取；整理前完整快照为[v0.5.2标签](https://github.com/JvKan9Zz/PortCanvas/tree/v0.5.2)。不改写历史提交。
