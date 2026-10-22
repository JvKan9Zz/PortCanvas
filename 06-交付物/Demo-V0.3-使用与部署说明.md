# Demo V0.3 使用与部署说明

本版在 V0.2 的连接、远程桌面和文件传输基础上，增加软件内置的右侧终端栏。运行 `demo-v0.3/win-unpacked/Wheel Combat Studio.exe`，保留整个目录。

## 使用终端

1. 在设备页连接树莓派。密码仅用于本次登录，不保存。
2. 进入桌面、文件或擂台页，点击右上角“终端”展开右侧栏，再点击“连接 / 恢复终端”。终端共用本次 SSH 登录，无需重复输入密码。
3. 点击深色终端区域即可输入指令。收起侧栏、切换页面会保持终端连接；设备页会隐藏侧栏，返回其他页面可继续使用。
4. “断开”仅关闭本次终端通道，远端会话保留。“清屏”只清理本机显示，不发送远端清理或结束程序的指令。
5. 断网、断开设备或关闭软件后，重新连接设备并点击“连接 / 恢复终端”，即可恢复该设备和账户的同一个会话。

需要持续运行的程序应在本软件终端中启动。会话由设备端 tmux 保存；设备需要继续供电且程序自身正常运行。设备关机或重启会结束内存会话。Ctrl+C 会中断前台程序，exit 会结束当前 Shell；这些操作仍按正常终端含义执行。

## 依赖与源码运行

- Windows 使用现有 Electron 桌面壳，终端由 xterm.js 渲染，不依赖 PuTTY 或外部终端窗口。
- 设备端需要 tmux。本次实机已安装 `tmux 3.0a-2ubuntu0.4` 和依赖 `libutempter0`；未升级或移除其他包，也未改 SSH、NoMachine、Xorg、SDK 或比赛代码。
- 其他设备可安装发行版提供的 tmux：`sudo apt-get install --no-install-recommends tmux`。
- 在 `03-实现与脚本/studio` 执行 `npm ci`、`npm run runtime`、`npm start`；测试 `npm test`，打包 `npm run pack`。
- 本机设备配置在 EXE 同级 `data/devices.json`。配置和本机测试证据不进入 GitHub 源码备份。

## 本版边界

擂台页面仍为后续入口。用户要求的左下角区域已在设计中调整为树莓派接口实时返回数值，后续依据真实接口数据实现，本版没有生成模拟数值。终端不自动重放输入或重启用户程序；重连恢复的是远端已存在的会话。

实现依据：[xterm.js API](https://xtermjs.org/docs/api/terminal/classes/terminal/)；[tmux 会话与断线说明](https://man.openbsd.org/OpenBSD-7.0/tmux.1)。
