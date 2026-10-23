# Demo V0.4 架构迁移记录

用户于 2026-10-23 授权正式完成底层框架迁移与重写、自行调试修复、GitHub PR。补充目标：保证完整功能和正确性，尽可能轻量、低占用且有普适性。性能是实测目标，不预设收益百分比；竞赛配置不能进入通用核心契约。不开发 V0.5 摄像头、赛台遥测或 AI/第三方插件。

基线：main / demo-v0.3.5 / 3e7c62aa788bac9ff86353e0a1268aa1803b8105。原 Electron 工程和旧 EXE 保留，新建 Tauri 工程；main 保持可运行。按规划先独立验证 M0/M1，再迁移 M2–M5；工作包分别提交和创建 PR，未获本次合并要求，不自动合并。

验收：设备资料/受信指纹兼容，SSH/桌面/SFTP/PTY/tmux/本地日志功能等价，旧数据可读且可回退；Rust 核心不依赖常驻 Electron/Node；自动测试、真实设备、Windows EXE、同条件全进程树资源对照、安装/便携交付。

初始环境：PowerShell Core 7.6.6、Bash 5.3.15、Node 24.19.0。Rust/Cargo 和 Visual Studio C++ 工具未发现；WebView2 155.0.4283.62 存在，Windows Kits 10/8.1 存在。第一次 Git 状态命令误在工作区根运行，无变更；随后在真实项目核对 main 干净、固定提交一致。

当前状态：M0/M1 已验证，准备独立 PR 后继续 M2–M5。所有失败、选型证据和后续结果在本文件追加。

用户随后明确：本轮完成后产品名 PortCanvas，使用所给 Logo，并同步现有 GitHub 仓库更名。旧内部协议/路径通过兼容层保留，不全局替换历史标识。

M0 已重新通过 38 项测试，并在 local/v04/electron-baseline 独立打包，未覆盖旧交付。T021 板端 Codex 101.2 秒完成，exit0/7 项命令/0 修改；静态核对 OpenSSH 8.2p1、Bash 5.0.17、Python3.8.10、tmux3.0a；沙箱 systemctl 权限不足不代表宿主服务不可用。x11vnc 不在 PATH，与当前旧版私有部署不同，后续独立复核旧路径，不安装新服务。

M1 排障：新工程误按依赖声明下限选了 noVNC1.6.0，该发布包仅有 CommonJS 且含顶层 await；尝试 core 入口也不存在，两次构建失败后停止查包。核实旧版实际锁定 1.7.0（官方 ESM exports），新工程对齐 1.7.0 后前端构建通过。Tauri API 类型需要 ESNext.Disposable，补足 TypeScript lib 声明；不改变运行目标。一次 Cargo 命令工作目录错误，修正 helper 指向 src-tauri。首次 Rust 链接误中 Git 的 link.exe，原因是 MSVC 安装尚未完成，等待工具链完成后再使用真实 MSVC 链接器，不改系统 PATH。

Windows SDK 首次落盘但未完成时再次链接缺 dbghelp.lib，停止编译并等安装完成；确认 MSVC/SDK 路径与实际库后构建成功。工具链子进程明确选择 MSVC，避免 Git link.exe 重名。setup 闭包错误类型从 anyhow 改为 Tauri 要求的 boxed error。Node REPL 没有 process 全局，GUI 启动改用正常 Python 子进程环境；这些均是本机开发入口，不进入应用运行依赖。

M1 最终：2 项 Rust 测试通过；真实 Rust SSH 探针验证错误指纹拒绝、正确指纹/认证/中文 exec/PTY/SFTP/direct-tcpip。原生 Tauri UI 的资料 IPC、中文、5 页、1000×680、xterm 实际按键/中文显示通过。合成 VNC fixture 首次错用 WebSocket 文本消息、随后未遵循客户端 SetPixelFormat，核对协议后改为二进制并按请求像素格式编码；最终逐像素/键鼠通过。证据 m1-rust-probe-result.json、m1-webview-result.json，不把合成组件测试当完整远程桌面验收。

本轮采集了 M1 调试原型测试后的 5 秒全进程树样本（m1-prototype-idle.json），包含额外组件测试模块/调试状态，仅探索记录，不能作为正式轻量化收益；M5 必须重新启动同功能发布包并与旧版比较。PortCanvas 原图和 imagegen 提取的透明图标存于 04-数据与素材/brand，原图未改，ICO 由官方 Tauri icon 转换；品牌正式切换在 M5。
