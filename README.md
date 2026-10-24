<p align="center">
  <img src="04-数据与素材/brand/portcanvas-readme.png" width="96" height="96" alt="PortCanvas Logo">
</p>

<h1 align="center">PortCanvas</h1>

<p align="center">统一远程设备工作台</p>
<p align="center">SSH 连接 · 远程桌面 · 文件传输 · 持久终端 · 本地运行日志 · 赛台监控</p>

PortCanvas 由 Wheel Combat Studio 发展而来，将远端 Linux 设备的连接、桌面操作、文件管理和运行调试放进同一个 Windows 客户端。项目从轮式格斗调试场景起步，正在将通用连接能力与竞赛业务分开，面向更多设备开发与远程调试场景。

## 版本状态

| 版本 | 状态 | 实现 |
| --- | --- | --- |
| **Demo V0.5.1 · PortCanvas** | 台面原图、放大赛台、移除尺寸标注、出发区齐平 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.5.0** | 当前main：[PR #15](https://github.com/JvKan9Zz/PortCanvas/pull/15)已合并；赛台、可开关远端摄像头、真实设备性能 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4.9** | [PR #13][v049-pr]、主题补充 [PR #14][v049-theme-pr] 已合并；设置、中英文与终端命令框/Tab补全 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4.8** | [PR #12][v048-pr] 已合并；简洁UI、统一按钮与拖拽传输 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4.7** | [PR #11][v047-pr] 已合并；多终端、手动重启/关闭、会话信息移出画面、清屏样式统一 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4.6** | [PR #10][v046-pr] 已合并，终端本地滚轮、侧栏调宽/动画、品牌禁选禁拖 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4.5** | 透明品牌、可选凭据和密钥认证基线，[PR #8][v045-pr] 已合并 | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.4** | 完整框架迁移与实机验收基线，[PR #7][v04-pr] | Tauri 2 + Rust + 系统 WebView2 |
| **Demo V0.3.5** | 保留的历史 Electron 版本 | Electron + ssh2 + noVNC |

当前工程位于 [`03-实现与脚本/studio-tauri`](03-实现与脚本/studio-tauri)，旧版 `studio` 和历史交付单独保留。

## 已实现的功能

| 功能 | 能力 |
| --- | --- |
| 设备连接与管理 | SSH 密码或私钥认证（OpenSSH / PEM / PPK），可选 Windows DPAPI 加密记住密码/密钥口令；主机指纹确认、取消连接与状态管理 |
| 远程桌面 | 进入设备已有的 X11 桌面，支持鼠标键盘、只读、缩放适配和全屏；桌面经 SSH 加密转发 |
| 文件管理与传输 | 本地/远端双栏浏览，本地盘符快速切换，面板间拖动上传/下载、资源管理器拖入上传，文件及目录双向传输、新建目录、进度、限速、取消与覆盖确认 |
| 右侧持久终端 | 在设备页以外展开，多标签独立运行，可手动重启或确认关闭；可拖动调宽、平滑开合；滚轮仅在本机滚动，收起或切页不结束程序，断线后恢复远端 tmux 会话 |
| 赛台监控 | 3.8m俯视赛台与2.4m台面，远端摄像头手动开关、默认关闭；CPU/内存/负载/网络等真实指标及60秒曲线 |
| 设置与语言 | 浅蓝/绿色/灰色主题、中英文即时切换并保存在本机、当前版本、仓库入口与版本检查；私有或网络不可访问时明确提示 |
| 电脑本地日志 | 按项目和日期记录命令、输出、退出码与中断状态；项目自动识别并允许手动指定，支持离线查看、筛选与清理 |
| 旧版资料迁移（V0.4） | 导入设备、受信指纹和日志设置；提供预览、冲突检查、备份及回退，保留原设备 ID |

桌面、SFTP 和终端使用独立通道。断开桌面不影响其他通道；终端中的程序由远端 tmux 承载，关闭客户端后可重新附着。设备断电、重启或程序自身退出不属于会话存活范围。

终端底部提供独立命令框：Enter发送、Shift+Enter换行、Tab补全远端命令名/路径，多候选先选后执行；每标签保留内存草稿与历史。

日志保存在**电脑本地**，采用 UTF-8 `run.json` 与 `output.log`，便于调试者或 AI 直接读取。终端脱离后，独立日志通道可继续采集；网络断开期间未收到的输出会明确标为缺失，不补造完整记录。V0.4 保持旧版 `wcs.run/1` 格式兼容。

关闭标签会确认终止对应Shell和前台程序，清屏及断开保留程序；全部关闭后不会自动重建。各终端可分别指定日志项目，会话信息显示在输入区上方。

## 开始使用

### Windows 安装与便携运行

[V0.5.1 发布包](https://github.com/JvKan9Zz/PortCanvas/releases/tag/v0.5.1)提供 Windows x64 安装包和便携包，附 SHA256SUMS.txt 校验文件。V0.1–V0.4.9 的 11 份发布包已放入 [GitHub Releases 历史归档](https://github.com/JvKan9Zz/PortCanvas/releases/tag/history-backup-2026-10-24)，下载后可按随附 SHA256SUMS.txt 校验。源码与验收文档保留在 Git 历史中，用户配置和日志不上传。

- [V0.5.1 使用说明](06-交付物/PortCanvas-Demo-V0.5.1-使用说明.md)
- [V0.5.1 验收记录](06-交付物/PortCanvas-Demo-V0.5.1-验收记录-2026-10-24.md)
- [V0.5.0 使用说明](06-交付物/PortCanvas-Demo-V0.5.0-使用说明.md)
- [V0.5.0 验收记录](06-交付物/PortCanvas-Demo-V0.5.0-验收记录-2026-10-24.md)
- [本地历史版本清理记录](07-日志与复盘/本地历史版本清理-2026-10-24.md)
- [V0.4.9 主题补充](06-交付物/PortCanvas-Demo-V0.4.9-主题补充说明.md)
- [V0.4.9 主题验收](06-交付物/PortCanvas-Demo-V0.4.9-主题验收-2026-10-24.md)
- [V0.4.9 使用说明](06-交付物/PortCanvas-Demo-V0.4.9-使用说明.md)
- [V0.4.9 验收记录](06-交付物/PortCanvas-Demo-V0.4.9-验收记录-2026-10-23.md)
- [V0.4.8 使用说明](06-交付物/PortCanvas-Demo-V0.4.8-使用说明.md)
- [V0.4.8 验收记录](06-交付物/PortCanvas-Demo-V0.4.8-验收记录-2026-10-23.md)
- [V0.4.7 使用说明](06-交付物/PortCanvas-Demo-V0.4.7-使用说明.md)
- [V0.4.7 验收记录](06-交付物/PortCanvas-Demo-V0.4.7-验收记录-2026-10-23.md)
- [V0.4.5 使用说明](06-交付物/PortCanvas-Demo-V0.4.5-使用说明.md)
- [V0.4.6 使用说明](06-交付物/PortCanvas-Demo-V0.4.6-使用说明.md)
- [V0.4.6 验收记录](06-交付物/PortCanvas-Demo-V0.4.6-验收记录-2026-10-23.md)
- [V0.4.5 验收记录](06-交付物/PortCanvas-Demo-V0.4.5-验收记录-2026-10-23.md)
- [框架迁移与旧资料导入说明][v04-guide]
- [V0.4 完整验收记录][v04-checks]
- [V0.3.5 使用说明](06-交付物/Demo-V0.3.5-使用说明.md)

打包客户端使用系统 WebView2，电脑端无需另外安装 Node.js、Rust 或 Python。普通安装使用当前用户的应用数据目录；便携模式将资料、缓存及默认日志放在 EXE 同目录的 `data/` 中。日志页显示实际保存位置，并支持更换。

记住凭据默认关闭。勾选后使用当前 Windows 账户加密；取消勾选并保存可移除。私钥文件只记录本地路径，内容不进入配置。跨电脑或 Windows 账户复制资料后需重新输入凭据。

### 从源码运行

开发需要 Node.js、Rust MSVC、Visual Studio C++ 工具链、Windows SDK 和 WebView2。在已配置工具链的开发终端执行：

```bash
git clone https://github.com/JvKan9Zz/PortCanvas.git
cd PortCanvas
cd "03-实现与脚本/studio-tauri"
npm ci
npm run dev
```

构建 Windows 安装包使用 `npm run pack`。工具链版本、测试命令和实现约定见 [V0.4 构建说明][v04-build]。

## 兼容与资源实测

本轮实测为 **Windows 11 x64** 客户端，以及基于 Ubuntu 20.04.6 / ARM64 的创意之星 AI 控制器（组内称“树莓派”）。远端需要 SSH/SFTP、Bash、tmux 与 Python 3；赛台摄像头还需要V4L2和OpenCV，默认关闭、手动开启，关闭/切页/最小化/断连/退出释放本次采集。远程桌面还需要已有的 X11 会话、授权和 x11vnc。其他 Linux 设备需满足相应条件，尚未完成全平台实机验收。

V0.4 将运行核心迁移到 Rust，并按需加载桌面和终端组件。完整发布包对照中，便携运行文件由约 **368.64 MiB** 降到 **11.32 MiB**（EXE 与许可说明，系统共享 WebView2 另计）；测试场景的私有内存中位数降低约 **5%–17%**。总工作集、CPU 和启动时间并非全面改善，测量方法、各场景数据与边界见 [资源实测报告][v04-performance]。

## 后续路线

赛台与可开关摄像头已在V0.5.0接入；实车定位、敌车/能量块、设备接口实时数值、AI和插件运行时留待后续开发。完整路线见 [统一远程工作台规划][roadmap]。

## 历史版本与资料

| 阶段 | 主要内容 | 文档 |
| --- | --- | --- |
| Demo V0.1 | 设备连接、嵌入式远程桌面 | [使用说明](06-交付物/Demo-V0.1-使用与部署说明.md) |
| Demo V0.2 | 双栏 SFTP 文件管理与传输 | [使用说明](06-交付物/Demo-V0.2-使用与部署说明.md) |
| Demo V0.3 | 右侧持久终端、断线恢复 | [使用说明](06-交付物/Demo-V0.3-使用与部署说明.md) |
| Demo V0.3.5 | 本地日志、项目/日期分类和清理 | [验收记录](06-交付物/Demo-V0.3.5-验收记录-2026-10-23.md) |
| Demo V0.4 | Rust/Tauri 重构、PortCanvas 品牌、可逆资料迁移 | [PR #7][v04-pr] |
| Demo V0.4.5 | 透明标志、可选保存凭据、密钥认证、本地盘符 | [使用说明](06-交付物/PortCanvas-Demo-V0.4.5-使用说明.md) |
| Demo V0.4.6 | 终端本地滚轮、侧栏调宽和动画、品牌禁选禁拖 | [使用说明](06-交付物/PortCanvas-Demo-V0.4.6-使用说明.md) |

项目编号为 `Project-016`，沿用 `00–07` 的管理、需求、设计、实现、素材、验证、交付和复盘目录。旧版 `03-实现与脚本/studio` 与 V0.4 的 `studio-tauri` 分开保留。设备凭据、私钥、用户日志、本地测试数据和依赖目录不提交到 Git。

[v04-source]: https://github.com/JvKan9Zz/PortCanvas/tree/feature/v04-portcanvas-release
[v04-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/7
[v045-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/8
[v046-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/10
[v04-guide]: https://github.com/JvKan9Zz/PortCanvas/blob/383dfcade64db0537da845a92ef1d6cd02d8509c/06-%E4%BA%A4%E4%BB%98%E7%89%A9/PortCanvas-Demo-V0.4-%E4%BD%BF%E7%94%A8%E8%AF%B4%E6%98%8E.md
[v04-checks]: https://github.com/JvKan9Zz/PortCanvas/blob/383dfcade64db0537da845a92ef1d6cd02d8509c/06-%E4%BA%A4%E4%BB%98%E7%89%A9/PortCanvas-Demo-V0.4-%E9%AA%8C%E6%94%B6%E8%AE%B0%E5%BD%95-2026-10-23.md
[v04-performance]: https://github.com/JvKan9Zz/PortCanvas/blob/383dfcade64db0537da845a92ef1d6cd02d8509c/06-%E4%BA%A4%E4%BB%98%E7%89%A9/PortCanvas-Demo-V0.4-%E8%B5%84%E6%BA%90%E5%AF%B9%E7%85%A7-2026-10-23.md
[v04-build]: https://github.com/JvKan9Zz/PortCanvas/blob/383dfcade64db0537da845a92ef1d6cd02d8509c/03-%E5%AE%9E%E7%8E%B0%E4%B8%8E%E8%84%9A%E6%9C%AC/studio-tauri/README.md
[roadmap]: https://github.com/JvKan9Zz/PortCanvas/blob/383dfcade64db0537da845a92ef1d6cd02d8509c/01-%E9%9C%80%E6%B1%82%E4%B8%8E%E7%A0%94%E7%A9%B6/%E7%BB%9F%E4%B8%80%E8%BF%9C%E7%A8%8B%E5%B7%A5%E4%BD%9C%E5%8F%B0-%E5%AE%8C%E6%95%B4%E5%BC%80%E5%8F%91%E9%9C%80%E6%B1%82%E4%B8%8E%E6%8A%80%E6%9C%AF%E8%B7%AF%E7%BA%BF%E8%A7%84%E5%88%92.md

[v047-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/11

[v048-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/12

[v049-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/13

[v049-theme-pr]: https://github.com/JvKan9Zz/PortCanvas/pull/14
