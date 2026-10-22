# Wheel Combat Studio · Demo V0.1

本次实现设备连接与管理、远程桌面。左侧文件和擂台入口保留，尚无对应业务功能。

## 启动

打开 `demo-v0.1/win-unpacked/Wheel Combat Studio.exe`。保留整个 win-unpacked 目录，不能只拷贝 EXE。无需在使用电脑另装 Node.js 或 Electron。

首次添加设备，填写设备名称、IP/主机名、SSH 端口（通常 22）、Linux 用户名和密码，点击连接设备。首次连接会显示 SSH 指纹，请与设备或已信任客户端核对。地址和用户名保存于 EXE 同目录 `data/devices.json`；密码只留在本次窗口内存，关闭软件后清除，不写入磁盘。不要在共享屏幕上显示密码。

设备连接成功后点击“进入远程桌面”，或侧栏“桌面”后点击“连接桌面”。共享已有 Linux 桌面，不创建新桌面。当前桌面用户需已登录，使用 X11；显示编号通常是 `:0`。默认先找当前 UID 的 `/run/user/<uid>/gdm/Xauthority`，否则使用该用户的 `~/.Xauthority`；可在桌面连接设置中指定授权文件路径。

画面中可使用鼠标与键盘。“适配”缩放到可用区域，“1:1”使用原始尺寸并允许滚动；“只读”停止向远端发送鼠标键盘；全屏按钮切换应用全屏。侧栏切页会保留桌面连接。“断开桌面”只关闭本次画面服务，设备 SSH 连接保留；设备页“断开设备”或关闭窗口会一并关闭本次桌面服务。设备断电/网络中断会显示断开，需要主动重新连接。

## 板端组件

本次已在创意之星 AI 控制器专用目录准备：

`/home/bcsh/robot-research/wheel-combat-studio/v0.1/runtime`

来源是板端已配置 Ubuntu apt 源的 x11vnc、libvncserver1、libvncclient1。只下载并在该目录解包，没有系统安装、开机服务或对 NoMachine/Xorg 的配置修改。每次桌面连接由独立 SSH 通道启动一个前台进程，只监听板端 127.0.0.1，经 SSH 加密转发；通道 EOF 后退出。关闭本软件不会注销 Linux 桌面。

若重装或换板，需要准备相同私有 runtime。开发脚本 `03-实现与脚本/prepare_board.py` 针对当前已有保存站点和 `/home/bcsh`，不会读取或导出 WinSCP 密码，不是任意设备通用安装器。新设备须先核实 SSH、用户、CPU 架构、系统库和 X11 会话，再调整路径/架构；不要直接运行历史清理脚本。

## 开发

应用源码位于 `03-实现与脚本/studio`。使用锁文件安装依赖：`npm ci`。新版 Electron npm 包需显式执行 `npm run runtime` 下载二进制；之后 `npm start` 启动，`npm test` 验证，`npm run pack` 生成 Windows x64 目录。GitHub 下载如受本机拦截影响，可为本次子进程配置现有 HTTP 代理；保留正常 TLS 校验。

使用 Electron、ssh2、ws、noVNC、IBM Plex Sans 等组件，依其各自许可使用。noVNC 和字体的原始许可随应用依赖/字体资源保留；后续公开分发前请保留对应源代码和许可文件。

## 当前边界

不含 WinSCP/SFTP、终端、相机、视觉定位和实车控制。本次不宣称与 NoMachine 的延迟、带宽或资源占用相当或更优。完整冷启动、长时间弱网与多设备兼容性需后续测试。
