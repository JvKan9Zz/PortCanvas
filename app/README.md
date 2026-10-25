# PortCanvas Demo V0.5.5

Tauri 2 + Rust + WebView2桌面客户端。Rust负责SSH、SFTP、桌面转发、PTY/tmux、本地日志、设置与按需摄像头；前端使用noVNC与xterm。Node仅用于构建，电脑端运行无需Node/Python。

## 构建

需要Node、Rust MSVC、VS C++ Build Tools、Windows SDK与WebView2。在本目录且MSVC环境已就绪时运行：

```bash
npm ci
npm run build
npx tsc --noEmit
npm run test:frontend
python -m unittest discover -s tests -p "test_desktop*.py"
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
npm run pack
```

Windows产物在src-tauri/target/release/bundle/nsis/。依赖版本以package-lock.json与Cargo.lock为准。其他平台尚无正式实机支持结论。

## 运行约定

默认使用当前用户AppData；同目录portable.flag或--portable启用同目录data/。WCS_DATA_DIR可供隔离测试指定资料目录。密码与密钥口令仅可选DPAPI加密保存，私钥只记本机路径；不提交凭据或用户数据。

终端由远端tmux保持，关闭标签会确认终止对应Shell，断开和清屏保留程序。日志只写电脑；网络缺口明确标记。SFTP传输保留覆盖确认和本次临时文件清理，未实现通用断点续传。远端删除与传输共用操作锁，确认目标后逐项删除，不跟随符号链接；中断后不重放删除。

摄像头优先用已有FFmpeg复制设备原生MJPEG，否则回退OpenCV；通过SSH与仅127.0.0.1的临时令牌通道显示，不录像。可选8/15/20/30 FPS，默认20；原生窗口最小化、切页、断连、退出暂停camera/pose，恢复不自动开摄像头。高帧率和较大原生JPEG会增加流量及部分PC占用。

桌面通过现有x11vnc/noVNC和SSH传输。流畅与清晰保留原始分辨率，省资源将传输画面按比例限制在960×540内；不改变X11屏幕模式。减少输入等待与发送延时，低成本缩放仅作用于省资源模式。桌面连接意图与可见性分开管理；切页/最小化停止本次服务，返回恢复；明确断开后不自动恢复。首次完整像素更新解码完成前显示阶段进度并屏蔽远端输入；首屏超时可重连。x11vnc 使用内置 lan 调度预设绕过同步测速造成的启动停顿，这不是限速或修改物理屏幕。摄像头仍遵循手动开启规则。

定位程序尚未接入。[接口约定](../docs/pose-protocol.md)采用全场左下角米制坐标与最新RAM JSON，超过2秒、无效或明确丢失定位的数据不显示。examples/publish_pose.py提供真实生产程序可调用的原子发布函数，不自动生成位置。

[用户说明](../docs/usage.md) · THIRD-PARTY-NOTICES.txt随安装与便携包分发。



## 更新发布

更新由 `tauri-plugin-updater` 校验签名。安装版使用 NSIS，便携版使用签名的 `PortCanvas.exe`，由该程序的更新助手在旧进程退出后替换；资料目录保留。`latest.json` 同时提供 `windows-x86_64`（安装器）与 `windows-x86_64-portable`（程序）条目。检查来源是 GitHub 最新 Release，未发布的 main 提交不提示升级。

正式打包须通过环境变量提供 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，私钥保存在仓库外部或已忽略的本机私密目录，不提交到 Git。构建生成的安装器及其 `.sig`、签名后的便携程序及其 `.sig` 和 `latest.json` 随 Release 上传；`bundle.createUpdaterArtifacts` 已启用。签名文件中的版本必须与该 Release 一致。

更新在用户点击后下载，校验完成后再确认安装并重启；下载可取消。安装前停止本机连接并封存日志，远端持久终端会话不主动终止。文件传输或资料迁移进行中不允许安装。便携目录必须可写。只有 debug 构建支持回环测试源，正式版固定使用 HTTPS 和内置公钥。

更新下载默认使用32路并发分段；服务不支持Range时自动回退到单连接。界面显示实际并发数，取消会停止本次下载。完整文件及版本签名通过校验后才能安装。
