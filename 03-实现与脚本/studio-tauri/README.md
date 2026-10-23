# PortCanvas / Demo V0.4 迁移工程

当前 M1 原型用于验证 Tauri2/WebView2、TypeScript 桥接和 Rust SSH；**尚非完整 V0.4**。正式操作继续使用旧 `../studio` 及 V0.3.5 EXE。后续按 M2–M5 迁移桌面、SFTP、终端、日志与资料兼容。

Node 只用于前端构建/测试，Rust/Tauri 承担 Windows 运行核心。`npm ci`、`npm run build`、`npx tsc --noEmit`；在 Rust MSVC/C++ Build Tools/Windows SDK 已配置的环境运行 `cargo test --manifest-path src-tauri/Cargo.toml` 和 `npm run pack`。设备探针 `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin compat-probe` 从 stdin 接收 profile/password/fingerprint JSON；不要把真实凭据写入命令行或普通日志。

原型默认使用用户可写 app data；本机测试通过仅子进程的 WCS_DATA_DIR/WEBVIEW2_USER_DATA_FOLDER 指向项目 local 隔离目录。核心 Tauri 命令仅授权 main 窗口和本地来源，显式 ACL，不暴露原始 Rust 句柄。M1 组件 fixture 仅在 WCS_M1_COMPONENT_TEST=1 构建，不由应用页面导入。

性能脚本 tools/sample_process_tree.py 需要 Python3+psutil，按整个进程树记录工作集、Private Bytes、CPU，原型与完整产品结果须分开。通用远端后端与竞赛配置解耦目标见项目迁移工作包。
