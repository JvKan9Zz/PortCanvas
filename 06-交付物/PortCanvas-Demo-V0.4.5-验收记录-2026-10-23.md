# PortCanvas Demo V0.4.5 验收记录

本版在 V0.4 完整 Rust/Tauri 工程上完成透明品牌资源、可选记住凭据、SSH 密钥认证与本地盘符切换。旧 Electron 源码及历史交付保留。验收环境为 Windows 11 x64 / WebView2 与 Ubuntu 20.04.6 ARM64 控制器。

| 检查 | 实际结果 |
| --- | --- |
| 核心测试 | 28 项 Rust 测试通过，0 失败；包含 DPAPI 真加解密、设备身份绑定、取消保存及 API 凭据脱敏，原有日志/迁移测试继续通过 |
| 构建 | npm 前端构建、TypeScript、原生 debug/release 与最终 NSIS 封装通过；未新增运行时 Node/Python 依赖 |
| 密码 | UI 勾选保存后磁盘仅有 DPAPI 密文；重启原生应用，空密码字段可连接；取消勾选并保存移除密文，空密码连接被拒绝；错误密码后可恢复 |
| 密钥 | 实际生成临时 Ed25519 OpenSSH 与加密 RSA PKCS8 PEM；中文/空格路径、错误口令恢复、RSA-SHA2、记住口令后重启连接均通过。PPK 由现有 russh 解码器支持，本次未额外做真实 PPK 登录 |
| 密钥通道 | 加密 RSA 会话双向传输中文文件逐字节一致；PTY/tmux 确认收到精确输出；独立日志初始化、桌面 SSH 转发启动/关闭通过 |
| 原生选择框 | 真实私钥选择窗口返回本机文件路径；选择无效私钥时给出格式/口令提示，设备页可继续操作；测试后恢复密码认证、不保存凭据 |
| 盘符 | Windows API 枚举实际 C/D 盘，UI 下拉框离线切换两个根目录成功；1000×680 视口无横向溢出 |
| 透明图标 | 使用用户指定图片原样作为源，1254×1254 RGBA；UI/README PNG 的四角 alpha=0，多尺寸 ICO 均为正方形，等比显示；侧栏不再有黑色方块底板 |
| 正式包 | release 原生程序真实 SSH/SFTP 回归通过；安装器升级成功，版本为 0.4.5，原设备和日志设置逐字节保持；安装目录仅应用、ICO、许可和卸载器 |
| 桌面入口 | 桌面 PortCanvas.lnk 指向实际安装 EXE，IconLocation 指向 portcanvas-0.4.5.ico；从快捷方式正常启动，窗口标题正确；不重启 Explorer 或清理全局缓存 |
| 便携交付 | 独立副本首次启动设备为空，日志在自身 data/logs；ZIP 白名单共 8 文件、解压自检通过，无用户数据；多尺寸 ICO 为 16/24/32/48/64/256 正方形；安装版与便携版代码/资源一致，仅保留官方 UNK/NSS 包类型标记差异 |

板端 Codex T022 只读任务 exit0、5 条命令、0 修改，先核实 SSH 公钥目录环境。随后真实密钥测试通过普通 SSH 临时添加仅限当前客户端地址的两条公钥。测试结束后确认 authorized_keys 精确恢复到原始不存在状态，临时私钥、测试目录和独立 tmux 会话已回收；未改 SDK、比赛代码、用户默认会话或运动硬件。只读沙箱的不可写判定没有当作实际 SSH 账户权限结论。

本次验证针对新增认证、资源和盘符行为。已有桌面鼠标键盘、传输覆盖/取消、日志断线和迁移等完整 V0.4 验收仍见历史记录；本版不重复声称做了全部压力或全平台验证。

原始本机证据在忽略目录 `05-测试与验证/local/v045/`：core-tests.txt、auth-result.json、key-cleanup-result.json、native-picker-result.json、release-smoke-result.json、installed-result.json、portable-result.json、package-result.json、release-build.txt、final-bundle.txt、release-files.png。T022 回收资料在 local/board/tasks 对应编号内。凭据和测试样本不进入 Git 或交付 ZIP。

GitHub：V0.4 总 PR #7 以 merge commit e4b7248 合入 main，#3 自动标记已合并。#4–#6 的头提交均已确认是 main 的祖先，因此作为被 #7 完整包含的分包关闭；它们不是单独再次合并。V0.4.5 PR #8 经检查后以 merge commit 6a7eaee 合入 main；仓库未配置 CI 检查，以上验证在本机完成。
