# Demo V0.4.8 开发记录

基于main d1db73d，分支feature/v048-clean-ui-drag-transfer。用户将本次精简说明、统一按钮、移除整条底部状态栏，以及新增拖拽传输合并为Demo V0.4.8。只改当前Tauri前端、拖入源列表所需文件服务、版本与交付说明；旧Electron及历史交付保留。全局工作台在完成后加入指定的UI文案原则。

按stop-that-shit和UI/UX Pro Max检查范围；本地UX库两次progressive disclosure查询不匹配，未将结果当作设计依据，直接以用户明确原则和既有控件语义实施。保留真实异常、标签与日志缺失提示，移除重复闲置说明。普通操作共用button/secondary/compact；导航和文件列表保留自己的交互角色。

Windows下Tauri原生拖入会接管WebView2的HTML5 DnD，因此保留原生OS文件路径事件，面板内部用PointerEvent拖动；不关闭系统拖入能力，不加入运行依赖。原生拖入路径扩展既有传输计划，继续验证特殊文件、目标冲突、覆盖确认、原子临时文件、取消与限速；同名多源在计划阶段拒绝，避免悄悄覆盖。

首次完整文档批量输出截断，分段补读；文件名假设main.js/devices*及default.json不存在，改用rg --files定位真实renderer.js和capabilities/main.json。Shell Core7.6.6/Bash5.3.15已实测。版本脚本的宽泛0.4.7替换误将wasm-bindgen-futures 0.4.79写成0.4.89，Cargo选择失败；精确恢复该依赖，仅应用版本更新，未尝试升级依赖或重复失败。

官方依据：https://v2.tauri.app/reference/config/#windowconfig 与 https://v2.tauri.app/reference/javascript/api/namespacewebviewwindow/#ondragdropevent 。验证、交付和PR结果随后追加。

## 实机结果

33项核心/TypeScript/前端/release/NSIS通过。面板PointerEvent多选上传/下载、递归目录/空目录、中文与二进制内容通过；Windows-MCP与Computer Use配合实际从Explorer拖入文件及目录，debug与release均通过，125%缩放坐标正确；正式输出哈希核对一致。两次原生覆盖确认（取消/确认）、限速取消、断线重连、混合父目录、同名冲突、符号链接拒绝及原栏落点均通过。没有修改或重启用户业务会话。

安全夹具把已隐藏侧栏scrollWidth当成可见溢出，未继续执行剩余UI断言。明确documentWidth=1000、可见节点无越界后，独立release UI夹具五页/认证字段/抽屉底边/无未命名按钮通过，未为错误口径改CSS。修正一个前端提交门控的finally提前释放问题，最终release验证采用该实现。完整失败证据保留。

板端入口第一次误用LUBY原桥，其request限定在LUBY目录内，未提交；后用项目既有board_task.py包装器，不扩大路径限制。该包装器没有--help解析，已读源码按实际action/name调用。T025已完成回收，exit0/8命令项/0变更；只读沙箱test-w失败不代表SFTP宿主权限，实际传输由电脑复核通过。一次sky状态请求text/screenshot都为false被拒绝，改用Windows-MCP截图，未据此盲点。

用户指定文案原则已写入D:/Codex-WorkSpace/全局工作台.md，原三份全局文档备份在本项目ignored目录；AGENTS/SOP职责和规则无冲突，未无故重复追加。真实测试目录及23个条目已逐项名单核对后回收。安装、交付和PR结果随后追加。

普通安装已升级0.4.8并重新打开；devices.json/log-settings.json逐字节保留（原无terminals.json），桌面快捷方式指向0.4.8 ICO。8文件ZIP自检通过，无data/用户资料；安装与便携代码仅官方包类型标记不同。便携副本首次设备为空、日志在自身data/logs，正常关闭。旧版0.4.7交付保留。

PR #12已合入main，merge commit `f44d5397bf6d53352d34416604d1ba1d5fefd440`，功能头`60d1f31b4ae2af6ee214d58f74984219290a75b0`。Code Review后端读取该头，checks/statuses/annotations为空；不代表外部CI或他人review通过。PR已关联当前Codex聊天，审查面板定位file-drag.js第55行，工具返回queued。合并匹配功能头，本地main已同步；后续仅补充交付记录。
