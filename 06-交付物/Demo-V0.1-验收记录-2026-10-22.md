# Demo V0.1 验收记录

日期：2026-10-22。结论：本次“设备连接与管理＋远程桌面”范围已实现，Windows x64 EXE 在当前创意之星 AI 控制器上通过真实画面、基本鼠标键盘、页面切换、只读、尺寸模式与断开重连检查。没有发现尚未修复的本次范围阻断问题。

## 本次交付

`demo-v0.1/win-unpacked/Wheel Combat Studio.exe`，同目录完整运行资源，以及源码 `03-实现与脚本/studio`。应用使用 Electron 42.11.18、ssh2、ws、noVNC；板端 x11vnc 私有运行时已准备。文件和擂台保留后续版本入口，无模拟业务数据。

## 验证结果

| 层级 | 实际检查 | 结果与证据 |
| --- | --- | --- |
| 自动化 | 本机 SSH fixture：凭据不落盘、真实 SSH/WebSocket 隧道收发、错误密码、主机身份拒绝、拒绝端口、取消后迟到验证、远端关闭清理 | 7/7 通过；源码 `test/session.test.cjs`，CLI 本轮执行输出 |
| 板端协议 | 当前 :0 的真实 VNC 握手、关闭通道 stdin 后退出和释放端口 | `05-测试与验证/local/desktop-lifecycle.json`：RFB 003.008、退出码 0、端口释放 true |
| Windows EXE | 从交付目录直接启动，读取已保存配置，输入本次密码，连接真实 SSH 和远程桌面 | 打包 EXE 普通窗口启动并显示真实 Linux 桌面；未依赖开发服务器 |
| 鼠标 | 双击远端桌面主目录图标打开文件管理器 | 真实远端窗口可见；没有打开终端或运行比赛代码 |
| 键盘 | Ctrl+L 聚焦远端地址栏，按 s，之后 Escape 撤销 | `screenshots/release-keyboard-s.jpg`；地址栏可见 s，撤销后原目录保留，未执行文件操作 |
| 只读 | 打开只读，在同一画布再次按 t | `screenshots/release-readonly-blocked.jpg`：地址栏仍为 s，模式提示为只读 |
| 页面与尺寸 | 设备/桌面来回切换、1:1 与适配来回切换 | `release-page-retained-fit.jpg`、`release-actual-size.jpg`；原 Linux 文件管理器会话保留，1:1 提供滚动 |
| 桌面断开 | 单独断开桌面，SSH 保持连接 | `release-desktop-stopped.jpg`；`vnc-processes-release-desktop-stopped.json` 中本软件 VNC 进程数为 0 |
| 桌面重连 | 再次打开桌面 | `release-desktop-reconnected.jpg`；仅一个新的本软件 VNC 进程，原 Linux 会话仍在 |
| 设备重连 | 完整断开 SSH，再用本次窗口内的凭据重新连接，并打开桌面 | `vnc-processes-release-device-disconnected.json` 中进程数为 0；`release-final-connected.jpg` 显示重连后的真实桌面 |
| UI | 正常窗口与 1000×680 CSS 视口、离线态、未来入口、辅助文字对比 | 独立终检两项均 resolved：辅助文字最低 4.62:1、稳定导航与主体一致；无横向溢出。小视口为 CSS 仿真，不是原生窗口尺寸验收 |

以上本机证据均在 `05-测试与验证/local/`，不提交普通仓库。截图使用的是交付 EXE 的真实窗口；离线 PNG 另有明确命名。

## 已发现并处理的问题

修复 Python 3.8 路径 API 兼容、noVNC 的包导出/顶层 await 构建问题、SSH 错误被关闭事件覆盖、桌面断流提示被主动断开文案覆盖、辅助文字对比不足。

生命周期探针第一次未持续读取 stderr，产生退出超时和一组本次探针残留进程。改为持续排空后退出码 0、端口释放通过；按父进程精确文件路径清理该探针对，不触及应用 SSH 内嵌会话。之后真实 EXE 的桌面断开与设备断开均独立核实为 0 个本软件 VNC 进程，不以探针结果代替。

## 验证边界

这是当前设备、当前网络的一次功能验收。没有做 NoMachine 同条件性能比较、长时间弱网/断电恢复、跨设备/Wayland 兼容、中文输入法/剪贴板、Ctrl+Alt+Del 或全屏操作的专门实测；不声称这些结果已通过。没有车辆运动、摄像头/NPU 测试、系统显示配置修改或性能优化。

用户在当前会话授权提供的密码仅用于登录，未写入源码、配置、压缩包或普通日志。软件关闭后本次内存凭据清除。最终可直接继续使用已连接的交付版窗口。
