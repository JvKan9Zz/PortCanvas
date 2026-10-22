# Demo V0.3.5 验收记录

日期：2026-10-23。设备为组内称“树莓派”的创意之星 AI 控制器；不运行车辆运动程序。

| 层级 | 结果与证据 |
| --- | --- |
| 自动测试 | `npm test` 38/38 通过；包含协议/中文分片、失败退出、本地分组、接续、路径范围/活跃保护、大文件预览、写入错误、异常退出遗留、配置错误，以及异步终端准备取消和会话身份。local/v035-tests.txt |
| 板端环境 | T020 由板端 Codex 完成，exit0；Bash 5.0.17、tmux 3.0a 等核实，无文件修改。local/board/tasks/T020-studio-log-audit-20261023/returned |
| 真实设备集成 | 中文 stdout/stderr 与 exit1；自动 cwd 分类及手动项目；交互终端断开后继续采集；后台 PID 与晚到输出；SSH 中断后同一记录接续、退出0但 complete=false；清理保留活跃记录。local/v035-integration-result.json |
| 打包 EXE | 实际 xterm 按键输入、日志中文输出/退出码详情、五导航、项目设置、失败状态筛选、设备离线读本地文件、原生取消保留/确认仅删选中测试日志、软件重启后离线读原记录；桌面连接和 SFTP 列表回归。local/v035-ui-result.json |
| 布局 | 正常与 1000×680 CSS 视口，日志页与右栏并排无横向溢出；窄区列表/详情上下排列。local/screenshots/v035-logs-final-small.png、v035-logs-final-offline.png |
| 打包一致性 | `npm run pack` 成功，`node tools/check-package.cjs` 验证 25 项源码/资源与 app.asar 一致。local/v035-package-check.json |
| 分发 ZIP | 105 个文件，158067835 字节，CRC 全部通过；未包含 data、连接配置或运行日志。local/v035-zip-result.json |

日志仅电脑落盘。真实测试在本项目 local 专用目录，远端只用独立 tmux 会话和无运动 Shell/Python 文本指令；集成测试结束销毁自身的隔离会话，原 V0.3 用户会话保留。板端没有创建本功能的运行日志文件，也未修改 SDK、比赛程序、Xorg 或 NoMachine。

本次没有做长时间压力测试、硬件运动验收或远程桌面性能对比。SSH 完全中断期间无法采集的输出明确标缺失；设备断电会结束 tmux 内存会话。普通命令日志覆盖外层 Bash 执行边界，后台及交互文本另查会话输出。
