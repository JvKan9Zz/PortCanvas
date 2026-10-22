# T018-studio-terminal-audit-20261022

用户授权为 Wheel Combat Studio 新增外部 SSH 终端及断联后远端程序保留，并要求独立分支和 PR。本任务只读核查，不写文件、不安装、不改配置、不运行硬件或比赛代码，不启动其他 Agent。

请使用有限命令报告 command -v tmux、tmux -V、当前用户及默认 shell；核实 /home/bcsh/robot-research/wheel-combat-studio 是否可访问；只列出本软件专用 tmux socket wcs-terminal 上已有的会话名称（若没有则注明），不读取其他用户终端内容。返回真实命令、退出码及未确认项。

当前业务目录 /home/bcsh/uptech_vision_ai_class_code/p_wheel_combat，自编根 jvk_wheel_conbat，其 debug_program、match_program、test_program 及两份不同 uptech.py 均不修改。系统优化、NCS2 和模型准备保持暂停。实际持久会话测试由电脑端用无运动的专用测试进程执行。
