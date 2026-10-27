# PortCanvas Demo V0.5.9

Tauri 2 + Rust + WebView2桌面客户端。Rust负责SSH、SFTP、桌面转发、PTY/tmux、本地日志、设置与按需摄像头；前端使用noVNC与xterm。Node仅用于构建，电脑端运行无需Node/Python。

## 构建

需要Node、Rust MSVC、VS C++ Build Tools、Windows SDK与WebView2。在本目录且MSVC环境已就绪时运行：

```bash
npm ci
npm run build
npx tsc --noEmit
npm run test:frontend
python -m unittest discover -s tests -p "test_*.py"
# Rust tests must use a dedicated existing directory (Git Bash on Windows).
mkdir -p src-tauri/target/local-tests
export WCS_TEST_ROOT="$(cygpath -w "$PWD/src-tauri/target/local-tests")"
cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
cargo test --manifest-path src-tauri/Cargo.toml --locked --no-default-features --test ssh_lifecycle
npm run pack
```

Windows 的上述示例在 Git Bash 中运行；在 macOS 上将 WCS_TEST_ROOT 设为该目录的绝对路径即可，无需 cygpath。CI 使用 PowerShell 7 并显式创建、传入隔离目录。Windows CI 对 main 和 PR 运行构建、类型检查、前端/Python/Rust 单测与 loopback SSH 回归，并以 --no-bundle 编译应用，不安装、不签名、不发布。

Windows产物在src-tauri/target/release/bundle/nsis/。依赖版本以package-lock.json与Cargo.lock为准。macOS V0.5.6使用独立配置和Universal DMG，最低目标macOS13。在Mac上运行 npm run pack -- --target universal-apple-darwin --bundles dmg；需要安装两个Rust目标。使用ad-hoc签名，不含Apple公证。macos-v056.yml只在本次适配分支或手动指定V0.5.6源码时运行，不跟随后续版本。

## 运行约定

默认使用当前用户AppData；同目录portable.flag或--portable启用同目录data/。WCS_DATA_DIR可供隔离测试指定资料目录。密码与密钥口令可选使用Windows DPAPI或Mac钥匙串保护的AES-GCM加密，私钥只记本机路径；不提交凭据或用户数据。Mac配置/默认日志位于应用资料目录，本地文件从用户主目录浏览。

终端由远端tmux保持，关闭标签会确认终止对应Shell，断开和清屏保留程序。日志只写电脑；网络缺口明确标记。SFTP传输保留覆盖确认和本次临时文件清理，未实现通用断点续传。远端删除与传输共用操作锁，确认目标后逐项删除，不跟随符号链接；中断后不重放删除。

摄像头优先用已有FFmpeg复制设备原生MJPEG，否则回退OpenCV；通过SSH与仅127.0.0.1的临时令牌通道显示，不录像。可选8/15/20/30 FPS，默认20；原生窗口最小化、切页、断连、退出暂停camera/pose，恢复不自动开摄像头。高帧率和较大原生JPEG会增加流量及部分PC占用。

桌面通过现有x11vnc/noVNC和SSH传输。流畅与清晰保留原始分辨率，省资源将传输画面按比例限制在960×540内；不改变X11屏幕模式。减少输入等待与发送延时，低成本缩放仅作用于省资源模式。桌面连接意图与可见性分开管理；切页/最小化停止本次服务，返回恢复；明确断开后不自动恢复。首次完整像素更新解码完成前显示阶段进度并屏蔽远端输入；首屏超时可重连。x11vnc 使用内置 lan 调度预设绕过同步测速造成的启动停顿，这不是限速或修改物理屏幕。摄像头仍遵循手动开启规则。

定位程序尚未接入。[接口约定](../docs/pose-protocol.md)采用全场左下角米制坐标与最新RAM JSON，超过2秒、无效或明确丢失定位的数据不显示。examples/publish_pose.py提供真实生产程序可调用的原子发布函数，不自动生成位置。

[用户说明](../docs/usage.md) · THIRD-PARTY-NOTICES.txt随安装与便携包分发。



## 更新发布

更新继续使用tauri-plugin-updater验证版本签名，仅支持Windows安装版。当前用户静默更新沿用注册表记录的位置；手动安装保留目录选择。便携模式保留旧资料读取兼容，但停止提供发布包或更新。Mac目前仅适配V0.5.6，后续可能等正式版推出后再继续适配，暂不跟随Windows后续Demo更新。

Windows构建通过TAURI_SIGNING_PRIVATE_KEY与TAURI_SIGNING_PRIVATE_KEY_PASSWORD提供既有更新密钥。只将安装器签名内容写入latest.json的windows-x86_64条目；不单发.sig、portable.bin、SHA256SUMS或说明ZIP。旧版更新器依赖releases/latest/download/latest.json，因此保留此清单。GitHub自动提供源码归档，Mac DMG只在v0.5.6发布。

更新在用户点击后下载，默认32路分段，不支持Range时回退单连接；可取消，完整文件通过版本签名校验后才允许安装。安装前停止本机连接并封存日志，远端持久会话保留；文件操作或迁移进行中不允许安装。正式版固定HTTPS与内置公钥。


远端依赖准备实现在 resources/prepare_environment.py 与 remote_env.rs，电脑下载/缓存位于 runtime_download.rs，复用 update_download.rs 的 32 路传输。按功能调用、连接内去重、取消/心跳看门狗及独立暂存发布；现有 SFTP 路径优先，缺组件才按当前账号补齐。软件缓存与系统安装分开，不运行维护脚本、不修改 SDK。EnvironmentSource 默认 Computer，设置可选 Board；每次准备在持有功能锁后快照下载来源。Computer 读取现有 APT 索引并经电脑 HTTPS 缓存和长度帧传包；Board 通过 apt-get download 下载同版本包；两者共用校验、解包和原子发布逻辑，取消通过 EOF/Close 回收本次准备。详见 ../docs/usage.md 的 V0.5.8 环境表。

界面统一入口为 ui/workspace.css。theme.js 负责主题色彩和 light/dark/system 外观，glass.js 映射非透明度，background.js 仅设置背景展示。settings.js 组合分组设置、ColorPicker 与 StorageSettings；偏好和背景受管副本由 preferences.rs 持久化。ui/fonts 内置三种字重的完整 PingFang WOFF2；许可边界见字体 NOTICE 和第三方声明。终端/原始日志用系统等宽字体加中文回退；顶部 wordmark 单独使用完整转换的 Macintosh Skeletal Filled Regular，来源、原许可及用户单独确认的分发权限见字体 NOTICE。

connection-indicator.js 提供4×4随机点阵与可重入异步等待提示，连接阶段保持同一图案，操作结束/失败收回。各页按已有busy状态或实际IPC Promise显示，下载/传输的真实进度仍保留；后台稳定采样不反复闪动。全局隐藏滚动条但不修改滚动权限。

背景裁剪由 background-crop.js/crop-model.js 负责选框与等比投影；background.js 将归一化范围映射到背景尺寸和位置。Native BackgroundDraft 将图片或视频复制为应用内临时文件，确认后原子保存受管原件及 backgroundCrop；取消不改旧设置并清理草稿。v0.5.9 使用正常递增版本，v0.5.8 可通过软件内检查更新升级。


V0.5.9：discovery.rs 从活动 IPv4 接口创建有界探测列表，只读取 SSH banner；discovery.js 维护发现列表与快捷登录，认证仍复用原 device:connect。preferences.rs 将导入草稿存为本地文件而非 RAM/Base64，background_media.rs 仅在 127.0.0.1 的随机端口/令牌路径服务当前背景或草稿，支持 GET/HEAD/Range，并以有界缓冲传输。background.js 管理背景、裁剪投影、视频音量及可见性；关闭裁剪或预览会卸载对应媒体。背景文件不进入安装包。
