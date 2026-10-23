# Demo V0.4 M0/M1 验证

本记录仅覆盖迁移可行性与基线，不把原型称为完整迁移。

- 基线 main/demo-v0.3.5：3e7c62aa788bac9ff86353e0a1268aa1803b8105，旧版 38 项测试重跑通过；独立打包在本项目 local/v04/electron-baseline，旧交付/配置/日志不覆盖。
- Windows 工具链：Rust 1.99.1 MSVC；VS BuildTools17.14.42，MSVC19.44.35229、Windows SDK10.0.26100.0；WebView2 155.0.4283.62。工具链安装是开发前置，不是应用发布时的常驻依赖。
- 选型：Tauri2 + TypeScript 受控桥接，保留现有页面；Rust russh0.64.1（ring/rsa）+ russh-sftp3.0.1。实际解析版本由 Cargo.lock/package-lock 固定。noVNC 对齐旧版实际1.7.0，xterm6.0.0/FitAddon0.11.0。
- 2 项 Rust 基线测试通过：旧设备校验和 JS 一致的稳定会话名；旧主机指纹结构序列化保留。前端构建及 TypeScript 检查通过。
- 真实设备探针：错误指纹拒绝；正确指纹认证、中文 exec、PTY、SFTP 目录读取、direct-tcpip 回环转发全部通过。仅运行无运动文本/回环测试，转发 fixture 限时且只绑定设备回环，不写远端文件或改服务。
- WebView2 原生窗口：Rust IPC 读取设备资料、中文、5 页面与1000×680布局通过；xterm 显示/真实按键及中文字符输入通过；本地明确标记的合成 VNC 测试完成 WebSocket 握手、逐像素核对、键鼠消息。它不是完整真实远程桌面迁移验收。
- 本机证据：local/v04/m0-tests.txt、m0-pack.txt、m1-core-test.txt、m1-rust-probe-result.json、m1-webview-result.json。板端 T021 completed/exit0/7项命令/0修改已回收复核；公开配置检查不替代真实客户端结果。

M1 结论：当前目标设备和 Windows 的基本链路可行，采用上述候选继续 M2–M5。主机证书认证目前不支持并显式拒绝；不由当前控制器通过推断全部 Linux/Wayland/Windows 远端兼容。

回退：旧 Electron 工程和 demo-v0.3.5 EXE/标签独立保留；M1 原型使用项目内隔离数据，不迁移或改写用户资料。核心功能迁移、真实完整桌面、双向文件错误/取消、日志恢复、正式资源收益和安装包仍待 M2–M5。
