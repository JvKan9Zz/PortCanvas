# T016-studio-desktop-audit-20261022

电脑端正在开发 Wheel Combat Studio demo V0.1。用户已授权开发、连接测试和必要电脑操控。本任务仅只读检查，禁止安装、修改配置、打开硬件、重启、退出桌面或运行比赛代码；不递归启动其他 Agent。

目标：给出本机现有桌面会话与可嵌入桌面方案的真实依据。检查当前用户、系统架构、DISPLAY/XAUTHORITY 的非秘密路径、Xorg/Wayland、活动桌面会话；command -v x11vnc x0vncserver Xvnc xrdp ffmpeg xdotool openssl；Python PIL、websockify、paramiko 是否存在；NoMachine 版本与监听端口；SSH/SFTP 服务；不打印任何密码、私钥、令牌或 Cookie。

允许只读系统元数据及 /home/bcsh/robot-research，/usr/NX 的版本/帮助信息，不读取认证内容。板端业务目录 /home/bcsh/uptech_vision_ai_class_code/p_wheel_combat 及自编 jvk_wheel_conbat/{debug_program,match_program,test_program} 保留，不修改两份 uptech.py。

输出具体命令、退出状态与观察结果。说明是否有可以共享当前 :0 会话的 VNC 服务/组件，若需安装给出最小组件建议但不要执行。只做环境检查，不跑性能测试，已暂停的优化/NCS2任务保持暂停。
