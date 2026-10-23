# PortCanvas Demo V0.4.6 验收记录

范围：终端滚轮改为本地滚动；侧栏可调宽、开合动画；品牌禁选禁拖。基于 V0.4.5，不改日志格式、业务代码和硬件。

| 检查 | 实际证据 |
| --- | --- |
| 核心与构建 | 28项原有Rust核心测试通过；TypeScript、前端构建、最终release和NSIS通过。未增加运行依赖 |
| 滚轮输入 | 真实SSH raw PTY读取，普通模式下4次上下滚轮收到0字节；仅在本次独立tmux会话启用mouse后，再4次滚轮也无新增字节。未过滤普通键盘输入 |
| 本地滚动 | 在测试渲染器添加明确标记的700px本地样本以形成溢出，滚轮在终端区域可改变侧栏scrollTop，远端仍无新增字节；结束移除样本。没有溢出时不宣称发生了可见滚动 |
| 普通按键与尺寸 | 真实文字M、上方向键、Z得到hex 4d1b5b415a，证明正常方向键保留；拖动后远端列数75→57、行数15→14 |
| 调宽 | 真实pointer拖拽及捕获释放通过；1500px视口Home/End为320/856px，方向键按24px调节；收起重开保留672px，窄窗口临时限制，扩大恢复 |
| 布局与动画 | 1000×680视口侧栏最大356px，文件区保留560px，无页面横向溢出；实际开合中间位置发生过渡，减少动态偏好时transition为0s；隐藏内容inert，不获取焦点 |
| 品牌 | Logo draggable=false，品牌user-select:none，dragstart被取消；真实鼠标拖动名称没有选中文字；终端user-select仍为auto，正文复制未全局禁用 |
| 正常升级 | 已正常关闭旧客户端并升级至0.4.6，原devices.json/log-settings.json逐字节保留；桌面快捷方式使用新版本ICO，普通安装版已重新打开 |

鼠标模式验证仅修改独立测试会话的mouse选项并随会话回收，未改全局设置或用户会话。板端Codex T023只读返回exit0、2命令项、0文件修改；受沙箱socket权限限制，实际tmux状态由电脑既有SSH独立复核。没有启动SDK、ADC、相机或比赛程序，测试输出是明确标记的终端样本。

前期仅用xterm自定义wheel回调返回false时，本地滚动未通过，因此未作为最终交付。最终版在容器捕获阶段接管，测试结果见pty-result.json；具体测试夹具失败及修正见开发记录，不把自动化脚本错误归因产品。

本机原始证据：`05-测试与验证/local/v046/` 下core-tests.txt、final-build-capture.txt、layout-capture-final.txt、layout-result.json、small-final.png、pty-capture-tests.txt、pty-result.json、terminal-final.png及T023回收资料。证据与测试日志不进入仓库或交付包。完整SSH/传输/日志的既有回归见V0.4/V0.4.5验收；本次未重复全部历史压力或全平台测试。
