# PortCanvas Demo V0.4.9 验收记录

范围为设置弹窗、中英文界面、固定GitHub版本检查、终端底部显示与独立命令框/Tab补全。旧Electron、历史交付、厂商SDK和用户业务代码未修改。

| 验证 | 结果 |
| --- | --- |
| 核心与构建 | 36项Rust核心通过，覆盖语言配置保存/损坏文件保留、更新版本比较及错误不冒充最新、模板翻译与用户路径保留；前端/TypeScript/native release及NSIS构建通过 |
| 命令框实机 | Enter、Send、Shift+Enter多行实际输出；独立标签草稿/历史、交互程序逐行及空行输入、IME确认不发送、滚轮不发字节、断开禁用/同PID恢复通过 |
| 语言 | 中英文即时切换，运行程序PID与输出流保持，中文程序原文保持；原生关闭终端确认的英文标题/消息与取消实测通过，未终止测试程序 |
| Tab补全 | 当前远端目录、空格路径引用、命令名、多候选方向键选择、不自动执行、输入修改后忽略旧结果通过；补全前缀不会被作为命令求值 |
| 底部黑条 | 定位为空旧xterm-viewport的黑色背景及溢出；只隐藏该旧层，保留有效屏幕和自定义滚动，新命令框置于输出区外 |

板端Codex T026只读完成（exit0、3命令项、0文件变更），核实Bash5.0.17/tmux3.0a/Python3.8.10与独立测试根。电脑端复核实际SSH/PTy执行，未调用SDK、传感器或运动硬件。所有测试仅使用自有命名空间和v049随机测试目录，按名单回收，不影响用户原有终端会话。

检查更新实测匿名私有仓库返回不可访问提示，不误报最新；不使用令牌、不改仓库可见性。公开版本比较使用响应夹具验证，当前不能声称公开仓库在线更新已验收。命令框只提供命令名/路径补全，不承诺别名、交互程序专用补全或任意Shell语法。

发现并修复：通用翻译模板吞后缀、语言保存后选择框焦点丢失、设置模态Tab焦点落到BODY。候选列表夹具曾在“正在补全”时过早断言结果，改为等待实际option；原生确认人工等待及Runtime回车转义失败记录保留。PTY含CSI重绘，剥除CSI后独立输出行复核通过，不把原始CRLF匹配失败误判为程序未执行。

原始证据在本机`05-测试与验证/local/v049/`：core-tests-final-source.txt、composer-result.json、command-output-result.json、completion-result.json及构建日志/截图。凭据、测试配置和用户日志不进入Git/分发包。正式布局、安装便携与PR结果在收尾时补记。

最终release双语言五页1000×680布局通过，无可见横向越界、无renderer异常；设置焦点回环/Esc回焦通过，未保存设备名/日志项目/命令草稿及名为“设置”的用户文件保持原值。`ui-result.json`与`ui-project-draft.txt`记录实际结果。设置对话框和终端命令框已保存截图。

最终release的9项补全检查通过，包含空格主目录路径，记录于`completion-release-final.txt`。仓库入口由应用实际调用并成功打开Chrome中的当前仓库。本机普通安装已升级0.4.9并重新打开，devices.json/log-settings.json/terminals.json逐字节保留（原无settings.json），桌面快捷方式指向0.4.9图标。

8文件ZIP自检通过，约7.0MiB，无用户data/日志/配置；安装与便携程序仅官方包类型标记不同。便携副本首次中文、设备为空、日志在自身data/logs，语言写入自身settings.json，正常关闭。ICO六个尺寸均为正方形，透明品牌资源沿用；旧版0.4.8交付保留。Windows11 x64验收不代表其他平台或全部历史功能压力测试通过。

PR #13已合入main，merge commit `1590408b74edb805faf6d667828b97fecba6fe02`，功能头`2074f384c27538ad82ef1f3b77f3ad96f2b4e624`。Code Review后端读取该头，checks/statuses/annotations为空，不代表外部CI或他人review通过。PR已关联当前聊天；审查面板定位新增command-completion.js第1行，工具返回queued。GitHub连接器按匹配功能头合并，本地main已同步，后续仅补充交付记录。
