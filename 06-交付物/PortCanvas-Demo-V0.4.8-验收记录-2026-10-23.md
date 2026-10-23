# PortCanvas Demo V0.4.8 验收记录

范围为UI文案精简、统一操作按钮、移除底栏和拖拽传输。旧Electron、历史交付、厂商SDK和用户业务代码未修改。

| 验证 | 结果 |
| --- | --- |
| 核心/构建 | 33项Rust核心通过，新增拖入源路径、多个父目录、去重和无效组合检查；TypeScript、前端、原生release/NSIS成功，无新增运行依赖 |
| 面板拖动 | 真实PointerEvent多选上传至远端文件夹、多选下载到本地，中文/空格名称、二进制内容及空目录核对通过 |
| 系统拖入 | Windows文件资源管理器实际多选文件+文件夹拖入；debug与正式release均收到原生事件并完成SFTP，正式输出逐文件哈希一致 |
| 覆盖 | 两次实际原生确认：取消保留目标，确认后安全替换并核对新内容 |
| 中断 | 1MiB/s限速拖拽上传，取消后目录无临时残留；传输中断开设备，重连后只回收已登记的本次临时文件 |
| 边界 | 混合父目录上传通过；同名多源在写入前拒绝，目标仍为空；拖动符号链接拒绝；拖回来源栏不产生传输 |
| 精简UI | 五个页面1000×680下可见内容无横向越界；底栏/侧栏版本标记/文字式操作按钮已移除，按钮名称和认证方式字段保留；终端用足底部空间，idle说明隐藏 |

板端Codex T025只读完成：exit0、8命令项、0文件变更，确认测试根与OpenSSH8.2p1安装包。沙箱test-w返回1，不能据此判定SFTP不可写；电脑端真实独立目录传输另行通过，未改权限。所有测试只用自有v048-随机目录，不启动SDK或硬件。

安全测试前六项实测通过后，UI夹具错误地把visibility:hidden侧栏的scrollWidth计入可见溢出，触发断言。复核documentWidth=viewport且可见节点不越界后，改为可见页面口径独立执行，最终release五页UI验证通过；原失败记录保留，不计为产品修复。真实目录及23个自有条目已核对名单后回收。当前没有跨平台、跨显示器或全部历史桌面/终端压力测试结论。

本机原始证据：`05-测试与验证/local/v048/`下core-tests-fixed.txt、transfer-result.json、safety-result.json、ui-result.json、release-result.json、cleanup-result.json、release-build.txt及截图。测试文件、凭据和用户日志不进入Git或交付包。

普通安装已升级0.4.8并重新打开；devices.json/log-settings.json逐字节保留（原无terminals.json），桌面快捷方式指向0.4.8 ICO。8文件ZIP自检通过，无data/用户资料；安装与便携代码仅官方包类型标记不同。便携副本首次设备为空、日志在自身data/logs，正常关闭。旧版0.4.7交付保留。

PR #12已合入main，merge commit `f44d5397bf6d53352d34416604d1ba1d5fefd440`，功能头`60d1f31b4ae2af6ee214d58f74984219290a75b0`。Code Review后端读取该头，checks/statuses/annotations为空；不代表外部CI或他人review通过。PR已关联当前Codex聊天，审查面板定位file-drag.js第55行，工具返回queued。合并匹配功能头，本地main已同步；后续仅补充交付记录。
