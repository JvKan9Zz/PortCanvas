# PortCanvas Demo V0.5.0

已完成M0–M5：Tauri 2 / WebView2前端，Rust承担SSH、桌面转发、SFTP、PTY/tmux、电脑本地日志与可逆资料导入。旧../studio和V0.3.5保留。正式支持当前Windows交付，其他平台没有实机验收结论。

V0.4.5增加密码/密钥认证切换（OpenSSH、PEM、PPK）、可选Windows DPAPI加密记住密码/密钥口令、本地盘符选择，并统一使用用户指定的透明Logo。私钥只保存本地路径，内容不复制到配置；取消勾选后保存即可移除已记住凭据。配置中的密文仅当前Windows账户可解密，API不向前端返回密文或明文。

V0.4.6在捕获阶段将滚轮限制为本机 buffer/侧栏滚动，保留实际键盘输入；侧栏支持拖拽、键盘调宽、双击复位及减少动态偏好的开合动画。品牌禁选禁拖仅作用于Logo与名称，不限制正文/终端复制。

V0.4.7增加按设备持久化的多终端目录、独立PTY/流ID/日志观察器；重启和关闭均确认目标，关闭最后一个后持久空列表。status仅会话内关闭，元数据放在应用内；清屏统一为描边按钮。

V0.4.8精简常驻说明/底栏，普通操作统一button/secondary/compact；保留必要错误与字段标签。Tauri原生系统拖入与PointerEvent面板拖动共存，外部源列表扩展既有传输计划，沿用覆盖确认、取消、临时文件与限速。

运行时无需常驻Node/Python；远端沿用Bash、tmux、Python3和X11/x11vnc辅助协议。Node只用于前端构建。默认恢复旧wcs日志会话、保留设备/指纹和wcs.run/1。WCS_SESSION_NAMESPACE仅用于隔离测试，不设置时兼容用户旧会话。

V0.5.0新增赛台：SVG毫米坐标、用户提供图纸的1024px显示贴图，真实/proc性能串行采样；远端V4L2/OpenCV视频经SSH传送有界JPEG，仅保留最新帧。摄像头默认关闭，手动开启；关闭、切页、最小化、断连、退出释放。原生消费租约与远端stdin/15秒心跳看门狗清理本次通道，不安装服务、不录像。实际车辆/能量块/敌车输入尚未接入。

## 构建

本轮工具链：Node24.19、Rust1.99.1 MSVC、VS Build Tools2022、Windows SDK10.0.26100.0。先按Tauri Windows要求安装C++工具链和WebView2，在正确MSVC环境运行：

```bash
npm ci
npm run build
npx tsc --noEmit
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
npm run pack
```

版本以package-lock.json/Cargo.lock为准。SSH兼容探针为开发示例：cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --example compat-probe，从stdin接收测试配置/密码/预期指纹；不得写真实凭据到源码、命令行或普通日志。探针不进入安装包。

## 运行与数据

PortCanvas.exe默认当前用户AppData目录；同目录portable.flag或--portable启用data/便携资料、缓存和日志。WCS_DATA_DIR兼容显式测试目录。日志位置在UI可见，用户日志设置优先。Node/Python采样器不是应用运行依赖。

noVNC/xterm首次使用才加载。二进制Tauri Channel通过消费ACK限制256KiB待消费输出，输入队列有界。SFTP覆盖上传仅在批准且支持OpenSSH posix-rename时原子替换；失败/取消保留最终目标，只清理本次临时路径，无通用断点续传。

日志独立于交互PTY；缺口续写同运行ID，complete=false保持。列表每页200条，详情最多256KiB尾部，完整文本在磁盘。资料导入预览/备份/冲突检查/回退，不搬移旧日志。原生确认默认取消；首次身份确认等待不计入网络超时，取消令牌拒绝过期信任。主窗口及本地来源由ACL和后端共同校验。

tools/sample_process_tree.py使用Python3+psutil比较完整进程树。验收与资源结果见06-交付物对应文档，不以Rust主进程或原型占用代替完整产品。许可说明THIRD-PARTY-NOTICES.txt随安装/便携包分发；Logo来源与修正记录见04-数据与素材/brand。
