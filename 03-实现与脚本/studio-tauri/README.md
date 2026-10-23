# PortCanvas / Demo V0.4 迁移工程

当前已完成M0–M3：Tauri2/WebView2、Rust SSH/完整桌面/SFTP/持久终端通过分包检查，**尚非完整 V0.4**。正式交付继续使用旧 `../studio` 及 V0.3.5 EXE；M4/M5继续日志、资料兼容及发布集成。

Node 只用于前端构建/测试，Rust/Tauri 承担 Windows 运行核心。`npm ci`、`npm run build`、`npx tsc --noEmit`；在 Rust MSVC/C++ Build Tools/Windows SDK 已配置的环境运行 `cargo test --manifest-path src-tauri/Cargo.toml` 和 `npm run pack`。设备探针 `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bin compat-probe` 从 stdin 接收 profile/password/fingerprint JSON；不要把真实凭据写入命令行或普通日志。

原型默认使用用户可写 app data；本机测试通过仅子进程的 WCS_DATA_DIR/WEBVIEW2_USER_DATA_FOLDER 指向项目 local 隔离目录。核心 Tauri 命令仅授权 main 窗口和本地来源，显式 ACL，不暴露原始 Rust 句柄。M1 组件 fixture 仅在 WCS_M1_COMPONENT_TEST=1 构建，不由应用页面导入。

终端通过受控的二进制Channel和消费ACK保持有界输出，默认恢复历史wcs日志会话；WCS_SESSION_NAMESPACE仅用于独立测试/高级会话隔离，未设置时保持旧命名。SFTP覆盖上传需要OpenSSH posix-rename扩展，批准后才替换；缺失时保留原文件。取消/失败只清理本次登记的临时文件，不提供崩溃后的通用断点续传。

性能脚本 tools/sample_process_tree.py 需要 Python3+psutil，按整个进程树记录工作集、Private Bytes、CPU，原型与完整产品结果须分开。通用远端后端与竞赛配置解耦目标见项目迁移工作包。
