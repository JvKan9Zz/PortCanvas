# PortCanvas

统一远程设备工作台，原Wheel Combat Studio。Demo V0.4采用Tauri 2、Rust和系统WebView2，实现设备管理、远程桌面、SFTP、右侧持久终端和电脑本地运行日志。

- GitHub：[JvKan9Zz/PortCanvas](https://github.com/JvKan9Zz/PortCanvas)，原私有仓库更名，历史与PR保留。
- V0.4已完成M0–M5及本机/控制器验收，源码位于studio-tauri，总PR供审查；本轮不自动合并，main/demo-v0.3.5保持稳定基线。旧Electron工程studio和历史交付保留。
- [安装包](06-交付物/demo-v0.4/PortCanvas_0.4.0_x64-setup.exe)、[便携EXE](06-交付物/demo-v0.4/portable/PortCanvas.exe)、[完整ZIP](06-交付物/PortCanvas-Demo-V0.4-Windows-x64.zip)仅在本机交付，未将设备配置、日志或二进制提交Git。
- [使用与迁移说明](06-交付物/PortCanvas-Demo-V0.4-使用说明.md) · [验收记录](06-交付物/PortCanvas-Demo-V0.4-验收记录-2026-10-23.md) · [完整进程树资源对照](06-交付物/PortCanvas-Demo-V0.4-资源对照-2026-10-23.md)。体积和私有内存降低；工作集、CPU和启动时间并非全面改善。

## 路线与实现

- [统一远程工作台规划](01-需求与研究/统一远程工作台-完整开发需求与技术路线规划.md)
- [V0.4工作包](02-方案与设计/Demo-V0.4-工作包与性能验收.md)
- [新工程构建](03-实现与脚本/studio-tauri/README.md)
- [迁移日志](07-日志与复盘/Demo-V0.4-迁移记录-2026-10-23.md)
- [项目简报](00-项目管理/项目简报.md) · [历史开发需求](01-需求与研究/开发需求.md)

Project-016目录名保持，00–07各管理目录沿用；新实现与旧实现分开保存。Windows11 x64和创意之星AI控制器已实测。通用核心与竞赛配置解耦；摄像头、AI、赛台接口数值和插件运行时按后续路线推进。

## Demo V0.3.5 交付

- 分支：`feature/local-run-logs`，包含 V0.3 的右侧持久终端基础；[PR #2](https://github.com/JvKan9Zz/wheel-combat-studio/pull/2) 面向 main，合并状态见 PR 页面。
- [Windows EXE](06-交付物/demo-v0.3.5/win-unpacked/Wheel%20Combat%20Studio.exe)（保留整个目录）
- [完整交付包](06-交付物/WheelCombatStudio-Demo-V0.3.5-Windows-x64.zip)
- [使用说明](06-交付物/Demo-V0.3.5-使用说明.md)
- [验收记录](06-交付物/Demo-V0.3.5-验收记录-2026-10-23.md)
- [日志方案](02-方案与设计/Demo-V0.3.5-本地日志方案.md)

日志默认保存在电脑“文档”下 `Wheel Combat Studio/Logs`，按项目/日期分组，每次运行有 UTF-8 `run.json` 与 `output.log`。支持自动识别或手动指定项目，程序失败和缺失输出明确标识；离线可读，清理会保留运行中的记录。后台及交错文本另存会话记录。树莓派不创建本功能的日志文件，断线期间未采集的输出不补造。

## Demo V0.3 历史分支交付

- 分支：`feature/persistent-terminal`，终端在桌面、文件、擂台页面右侧展开，设备页隐藏；收起或切页保持连接，断线后可恢复远端 tmux 会话。
- [Windows EXE](06-交付物/demo-v0.3/win-unpacked/Wheel%20Combat%20Studio.exe)（保留整个目录）
- [使用与部署](06-交付物/Demo-V0.3-使用与部署说明.md)
- [验收记录](06-交付物/Demo-V0.3-验收记录-2026-10-23.md)
- [终端与擂台接口区设计](02-方案与设计/右侧持久终端与擂台接口区.md)

25 项测试、真实输入/中文输出、侧栏布局和同 PID 断线/重启恢复通过。设备端需要 tmux。擂台左下角的真实接口值区记录为后续设计，当前不显示模拟硬件数值。

## Demo V0.2 已备份版本入口

- [Windows EXE](06-交付物/demo-v0.2/win-unpacked/Wheel%20Combat%20Studio.exe)（整个目录保留在一起）
- [完整交付包](06-交付物/WheelCombatStudio-Demo-V0.2-Windows-x64.zip)
- [使用说明](06-交付物/Demo-V0.2-使用与部署说明.md)
- [验收记录](06-交付物/Demo-V0.2-验收记录-2026-10-22.md)
- [文件传输方案](02-方案与设计/demo-v0.2-文件传输方案.md)
- [本轮开发日志](07-日志与复盘/demo-v0.2-开发日志-2026-10-22.md)

验证包括 16 项自动化测试、真实树莓派双向内容哈希/覆盖/权限/取消/断线检查，以及交付 EXE 的目录操作、上传下载、原生覆盖确认与桌面并行回归。测试数据位于本项目 local 证据目录及板端 wheel-combat-studio 的专用测试目录。

GitHub 备份包含源码、依赖锁文件、测试、设计文档和使用/验收说明。EXE、完整交付 ZIP、依赖目录、设备配置及本机测试证据保留在本机，不进入 Git。

## Demo V0.1 历史交付

- [Windows EXE](06-交付物/demo-v0.1/win-unpacked/Wheel%20Combat%20Studio.exe)（整个目录保留在一起）
- [使用与部署说明](06-交付物/Demo-V0.1-使用与部署说明.md)
- [验收记录](06-交付物/Demo-V0.1-验收记录-2026-10-22.md)
- [范围与实现](02-方案与设计/demo-v0.1-范围与实现.md)
- [开发日志](07-日志与复盘/demo-v0.1-开发日志-2026-10-22.md)

源码运行：在 `03-实现与脚本/studio` 执行 `npm ci`、`npm run runtime`、`npm start`；测试 `npm test`；打包 `npm run pack`。所有开发命令仍遵守工作区的 PowerShell 7 / Bash 入口。
