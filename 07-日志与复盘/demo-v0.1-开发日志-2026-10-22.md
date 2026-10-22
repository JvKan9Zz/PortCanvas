# Demo V0.1 开发记录

范围：用户先要求桌面＋传文件，随后明确本次只做左侧导航的前两部分（设备连接管理、远程桌面）。文件与擂台只保留后续版本入口。技术栈委托选择，直接编码，授权电脑操控、必要下载和安装。

## 环境与准备

- 首次实测 PowerShell Core 7.6.6、Git Bash 5.3.15，固定 pwsh 外层启动 Bash。
- 复用现有保存的 WinSCP 站点，不读取或导出密码。
- T016-studio-desktop-audit-20261022 板端 Codex 因 workspace routing discovery timed out 失败，未执行命令、未修改文件。停止同通道模型重试；电脑通过既有 WinSCP 执行确定性的有限检查与项目私有组件准备。保留原始任务和 transport 证据。
- 板端未安装 x11vnc。通过系统已配置 apt 源只下载 x11vnc/libvncserver1/libvncclient1 的 ARM64 deb，在 /home/bcsh/robot-research/wheel-combat-studio/v0.1/runtime 解包，未安装系统包、未变更现有 NoMachine/Xorg 配置。ldd 无缺库，x11vnc 版本命令通过。
- 初次准备因 Python 3.8 无 pathlib.Path.is_relative_to 失败；改用 relative_to 后成功。错误属于脚本 API 兼容性，与 SSH 和 shell 无关。
- Impeccable 方向脚本首次缺 PRODUCT.md 拒绝运行；确认用户选择并建立产品记录后执行成功，Windows Node 脚本退出时又报 UV_HANDLE_CLOSING 断言。用户明确指定固定导航和功能优先，因此界面按其布局直接实现，不扩大到视觉方案审批或技能更新。

当前阶段：实现及测试中，未把组件准备或构建视为真实桌面验收。

## 构建与代码修复

- noVNC 当前 npm 包仅导出包根，并使用顶层 await；先后将导入改为包根、构建改为 ESM、HTML 改为 module，最终构建通过。没有屏蔽依赖错误。
- Electron 42 npm 包需显式运行 install.js 下载运行时。首次直连 fetch 失败；核实当前 127.0.0.1:10808 代理仍监听后，仅在下载子进程设置 NODE_USE_ENV_PROXY/HTTPS_PROXY，保持 TLS 校验，下载及打包通过。没有修改系统代理或关闭证书校验。
- 修复 SSH 错误被 close 事件覆盖、桌面断流提示被主动断开提示覆盖，以及桌面服务手动退出返回非零的问题。
- 7 项本机测试覆盖真实 ssh2 客户端/测试服务端握手、VNC WebSocket 隧道收发、密码错误、主机身份拒绝、端口拒绝、取消后迟到验证、远端关闭与资源回收；测试认证均为显式 fixture 数据。
- 真实板端 :0 返回 RFB 003.008；关闭 stdin 后专属服务退出、监听端口释放。仅证明协议与生命周期，不冒充最终应用鼠标键盘验收。
- Electron 页面 CDP 不支持 Browser.getWindowForTarget；改用 Emulation 的 CSS 视口检查，未继续猜 Browser 命令。1000×680 CSS 视口允许页面纵向滚动，未横向溢出；该结果不是原生窗口缩放测试。
- Impeccable detector 因缺解析器退化为正则，返回空发现列表，仅作有限参考。保留已导出的界面截图，独立审查其可读性和范围。

当时的真实登录等待用户在软件内输入一次设备密码；没有从 WinSCP 导出密码。

## 实机收尾

用户随后确认已连接，并授权后续自行输入密码。密码只用于本次软件登录，没有写入文件或普通日志。使用打包后的 EXE 完成真实画面、鼠标双击、Ctrl+L/s/Escape、只读阻止 t、切页保留、1:1/适配、桌面断开重连和完整设备断开重连。

退出探针出现 stderr 管道未排空导致的超时，补充后台读取后退出码 0、端口释放通过；精确清理探针对 6989/6987，应用内嵌 SSH 会话未触及。之后真实 EXE 两种断开均核实专属 VNC 进程数为 0。记录在 `probe-cleanup.json`、`vnc-processes-*.json`。

Computer Use 首次关闭窗口提示用户输入导致引用过期，重新获取状态后才继续；关闭窗口异步完成后重新枚举。窗口工具 WaitFor 未识别已经显示的 Electron 文本，以目标窗口的新截图和控件树复核，未把工具等待失败当成连接失败。远端 `type_text` 没有写入画布，改用真实按键事件验证基本键盘输入，不以批量粘贴测试代替按键验收。

UI 独立终检发现辅助文字对比不足和截图过渡帧问题，一次修正并复核，两项均 resolved，不再开展泛化抛光。真实输入验证在该审查之后完成，证据与限制见交付验收记录。
