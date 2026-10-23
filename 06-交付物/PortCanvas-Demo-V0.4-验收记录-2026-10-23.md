# PortCanvas Demo V0.4 验收记录

底层由Electron/Node迁移到Tauri 2 + Rust + 系统WebView2，Windows运行核心无需常驻Node/Python。新工程studio-tauri与旧studio分开，V0.3.5源码、main基线、标签和历史交付均保留。现有私有GitHub仓库已更名PortCanvas。

## 完成范围

设备资料和主机身份、SSH连接/取消/保活、已有X11桌面、双向SFTP、持久PTY/tmux终端、独立本地日志、旧资料可逆导入、PortCanvas品牌与方形图标、Windows安装与便携交付。内部wcs协议、日志schema、设备ID、项目散列和默认终端身份兼容。

noVNC/xterm按需加载，独立终端通道有消费ACK和256KiB待消费配额；文件传输流式临时写入再提交，日志按页读取、详情限256KiB尾部。没有为降低数字关闭功能、GPU或安全检查。

## 已通过的检查

| 检查层级 | 结果与证据 |
| --- | --- |
| 旧版冻结 | M0重新通过38项旧版测试，独立打包；main/demo-v0.3.5固定基线3e7c62a |
| 板端兼容 | T021板端Codex返回exit0，7条只读检查，0修改；Ubuntu20.04.6/aarch64/OpenSSH8.2p1/Bash5.0.17/Python3.8.10/tmux3.0a；电脑端再用真实Rust SSH/PTY/SFTP/direct-tcpip复核 |
| 核心与构建 | 最终26项Rust核心测试通过；前端打包、TypeScript、原生release、NSIS构建通过；开发探针作为Cargo example可独立构建，不进入安装包 |
| 身份与连接 | 错误指纹、错误密码拒绝后可恢复；首次确认取消不保存指纹；真实等待超过一分钟后仍能信任并连接，正常网络阶段仍有30秒超时 |
| 真实桌面 | WebView2/noVNC/Rust转发到现有X11，真实测试窗口收到鼠标与KEY=97；只读阻止键鼠，适配/原始尺寸/全屏、独立关闭、取消与重连通过；错误Origin403 |
| 传输 | 双向中文、嵌套/空目录、新建目录及内容哈希一致；权限失败可恢复、链接拒绝、双向取消、断线重连清理自有临时路径；原生覆盖取消/确认与确认后外部变化保护通过 |
| 日志 | 中文stdout/stderr、exit1、自动/手动项目、后台会话输出、终端脱离仍记录；断线同PID/同运行ID续写并标缺失，离线读取、活跃清理保护和真实取消通过 |
| 应用异常退出 | 强制结束Windows主程序及其6个自有WebView子进程，远端程序PID12560存活；远端自有x11vnc回到0；重启后原记录标中断，同会话/日志ID继续写入 |
| 资料兼容 | 原样V0.3.5日志可读；原生导入取消不写、确认保留ID/指纹/来源；回退两份配置逐字节一致，历史日志不删除；损坏回执和变更冲突拒绝 |
| 便携包 | 从独立可写目录正常启动，data/logs与WebView缓存留在同目录；初始页面无桌面/终端JS分块；205份明确标注的合成分页样本在UI按200/5分两页，往返通过；1000×680视口无横向溢出 |
| 最终图标 | 用户PC图形配全幅方形底板；源PNG1254×1254，UI资源128×128按48×48显示；侧栏旧W已替换，窗口/任务栏多尺寸ICO同步 |
| 正常安装 | 当前用户NSIS安装、卸载测试与重装通过；只有应用EXE、许可说明、卸载器；正常%APPDATA%目录读写，原有设备creative-star通过原生入口导入，实际SSH/SFTP与桌面通道验证通过 |
| 最终文件一致性 | 安装版与便携EXE仅相差Tauri官方UNK/NSS包类型标记的3字节，其余代码/资源完全一致；已核对tauri-utils 2.10.2源码，未修改二进制 |

真实设备测试使用独立tmux身份和v0.4-tests专用目录，已回收本轮会话、测试进程及远端测试文件；未操作用户默认tmux会话、SDK、比赛代码或运动硬件。分页样本只在本项目local隔离目录，未进入用户日志或交付ZIP。最终正常安装版已导入原设备配置并以普通模式打开，密码未保存。

## 资源结果与边界

同条件完整发布包矩阵已完成，详见资源对照文档。EXE+许可说明约11.32MiB，旧便携运行目录约368.64MiB；最终安装器约2.83MiB，系统共享WebView2另计。私有内存中位数在本次场景降低约5%–17%；总工作集部分场景更高，终端/并行CPU和启动时间也未全面改善。不能据此声称Tauri在全部指标优于Electron。

每个活动场景2轮各6秒，断开后另观察60秒，不替代长时间稳定性试验。支持边界为当前Windows11 x64与实际控制器环境；其他Linux须具备SSH/SFTP、Bash/tmux/Python3及对应X11/x11vnc条件，未做全平台实机验收。离线未收到的输出不补造；设备重启不能保证tmux存活。Wayland、Windows远端、摄像头、AI、赛台真实接口值和插件运行时不在本版完成范围。

## 交付与追溯

安装器和portable位于demo-v0.4，完整ZIP按白名单封装，不含data、日志、凭据或测试样本。源码及锁文件通过feature/v04-portcanvas-release的总PR供合入main审查；M0–M4分包PR #3–#6保留作检查索引。本轮未自动合并，main保持V0.3.5稳定版。

本机原始证据位于忽略目录05-测试与验证/local/v04：m5-core-test.txt、m5-final-build.txt、m5-live-result.json、m5-transfer-result.json、m5-logs-result.json、m5-first-trust-result.json、m5-crash-result.json、m5-portable-result.json、m5-installed-result.json、m5-installed-binary-result.json、performance/。M2–M4具体记录提供原生覆盖、清理、导入和回退证据。失败脚本、修正原因与恢复路径见迁移日志。
