# T021-v04-ssh-compatibility-20261023

背景：电脑端把 Wheel Combat Studio 从已验证的 Electron V0.3.5 迁移至 Tauri 2/Rust。本任务仅只读核对当前目标 Linux 控制器的 SSH/SFTP/桌面/终端前置条件，为 Rust SSH 选型提供依据。

范围：读取系统/SSH 服务版本、可读的公开 sshd 配置中算法/认证相关字段；Bash/Python/tmux/base64/git 的实际版本；已安装 x11vnc 文件与 X11 环境公开路径。不能读取密码、私钥、Codex 凭据或完整用户日志；不能修改配置/安装软件/重启服务/打开硬件/运行比赛或传感器程序。不要递归启动其他 Codex。

公共 SDK、两份不同的 uptech.py、视觉/模型/NPU 环境和已有 tmux 会话保持。业务根 /home/bcsh/uptech_vision_ai_class_code/p_wheel_combat；自编目录 jvk_wheel_conbat 中 debug_program/match_program/test_program，本任务不执行这些目录代码。此前性能/NCS2 工作暂停，不恢复。

返回：实际命令与输出、确认可用/不可用/未知项、是否存在配置读取权限限制、修改数（应为 0）。静态版本检查不等于新 Rust 客户端实机验证；不要推断未执行能力已通过。
