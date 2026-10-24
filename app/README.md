# PortCanvas Demo V0.5.3

Tauri 2 + Rust + WebView2桌面客户端。Rust负责SSH、SFTP、桌面转发、PTY/tmux、本地日志、设置与按需摄像头；前端使用noVNC与xterm。Node仅用于构建，电脑端运行无需Node/Python。

## 构建

需要Node、Rust MSVC、VS C++ Build Tools、Windows SDK与WebView2。在本目录且MSVC环境已就绪时运行：

```bash
npm ci
npm run build
npx tsc --noEmit
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
npm run pack
```

Windows产物在src-tauri/target/release/bundle/nsis/。依赖版本以package-lock.json与Cargo.lock为准。其他平台尚无正式实机支持结论。

## 运行约定

默认使用当前用户AppData；同目录portable.flag或--portable启用同目录data/。WCS_DATA_DIR可供隔离测试指定资料目录。密码与密钥口令仅可选DPAPI加密保存，私钥只记本机路径；不提交凭据或用户数据。

终端由远端tmux保持，关闭标签会确认终止对应Shell，断开和清屏保留程序。日志只写电脑；网络缺口明确标记。SFTP传输保留覆盖确认和本次临时文件清理，未实现通用断点续传。

摄像头优先用已有FFmpeg复制设备原生MJPEG，否则回退OpenCV；通过SSH与仅127.0.0.1的临时令牌通道显示，不录像。可选8/15/20/30 FPS，默认20；原生窗口最小化、切页、断连、退出暂停camera/pose，恢复不自动开摄像头。高帧率和较大原生JPEG会增加流量及部分PC占用。

定位程序尚未接入。[接口约定](../docs/pose-protocol.md)采用全场左下角米制坐标与最新RAM JSON，超过2秒、无效或明确丢失定位的数据不显示。examples/publish_pose.py提供真实生产程序可调用的原子发布函数，不自动生成位置。

[用户说明](../docs/usage.md) · THIRD-PARTY-NOTICES.txt随安装与便携包分发。

V0.5.3使用GitHub releases/latest检查正式发布版本，不再读取main中的package.json；草稿/预发布和限流/不可访问状态分别处理。目录、文档与素材使用英文路径。
