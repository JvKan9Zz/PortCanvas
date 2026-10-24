---
version: 1
slug: "03-studio-tauri-frontend-arena-page-js"
primary_target: "03-实现与脚本/studio-tauri/frontend/arena-page.js"
related_targets: ["03-实现与脚本/studio-tauri/ui/index.html","03-实现与脚本/studio-tauri/ui/arena.css","03-实现与脚本/studio-tauri/frontend/arena-model.js","03-实现与脚本/studio-tauri/frontend/en.json","03-实现与脚本/studio-tauri/src-tauri/src/arena_pose.rs","03-实现与脚本/studio-tauri/src-tauri/resources/pose_stream.py"]
---

# V0.5.2 赛台表面

- 模式：Operate；在既有 PortCanvas 工作区内操作。
- 范围：`frontend/arena-page.js` 驱动的赛台页；源码路径均相对 `03-实现与脚本/studio-tauri`。方向契约为 `ui/index.html` 首个 body 注释 `V052-ARENA CONTRACT`。
- 使用者与任务：开发者连接真实 Linux 设备，选择预览帧率，同时观察真实系统性能、全场坐标系与可用的己方位置输入。
- 主动作：连接设备后选择视频设备与 8 / 15 / 20 / 30 FPS，再手动开启摄像头；默认 20 FPS、摄像头关闭。关闭会释放本次采集，采集/关闭过程中禁用帧率选择。
- 生命周期：切页、最小化、断连和退出停止本次相机与定位观察；恢复可见赛台页且连接有效时恢复指标与定位观察，相机仍须手动开启。
- 构图：常规宽度左侧约三分之一为摄像头在上、性能在下，右侧约三分之二为赛台；在窄主内容区重排并滚动，保留完整状态、数值和单位。
- 方向：用户明确构图的代码实现，沿用灰白表面、浅蓝默认主题、Plex/中文回退、线性图标与带边框紧凑按钮；未建立新视觉系统。
- 核心内容：围墙内 3.8 × 3.8 m，中央台面 2.4 × 2.4 m，台下宽 0.70 m，用户工作区原图的 1536 px PNG（含 ICC）与红围墙。左黄上边/右蓝下边与台面齐平。原点为全场左下角，X 右/Y 上、单位米、范围 0–3.8，台上台下共用；新增坐标刻度与方向，不恢复尺寸辅助线/图例。
- 摄像头：640 × 480 预览；支持原生 MJPEG 且有 FFmpeg 时复制压缩帧，否则使用 OpenCV。现有 SSH 加密到电脑，随机令牌认证的 127.0.0.1 临时 MJPEG 通道供 img 显示，IPC 只传元数据；不录制。
- 指标：真实 Linux 指标与最近 60 秒 CPU/内存曲线来自 SSH；与相机开关独立。
- 记忆点：手动开启的真实画面与大幅等比例赛台并置；信息密度服从观察任务。
- 定位输入：只读观察远端 `/dev/shm/portcanvas-pose.json`，schema `portcanvas.pose/1`，heading/confidence 可选。有效新鲜输入才绘制己方点位与两位小数 X/Y；缺值、无效、超过 2 秒或 valid:false 时隐藏点位并显示“—”，缺少 heading 时隐藏朝向箭头。
- 约束与未决项：用户确认当前没有定位生产程序，正常显示“定位未接入”；不生成位置、不宣称自主定位或精度。敌车、能量块与比赛传感器接口仍未接入。协议见 `02-方案与设计/Demo-V0.5.2-定位数据接口.md`（相对项目根）。
- 证据边界：保存的 finish review 为 ship、无实质问题，覆盖 desktop/minimum/english-minimum/terminal-open/synthetic-pose 五张图；合成点位明确标为协议示例，不能当实车观测。短测默认 20 FPS 约交付 19.95 FPS、板端 CPU 降低，电脑私有字节/CPU 与流量增加，不声称总资源均下降。

## 历史 V0.5.1

V0.5.1 不显示尺寸数字、辅助线或尺寸图例，收紧画布；黄色上边与台面上边齐平，蓝色下边与台面下边齐平，内部几何比例保持。当时摄像头上限 8 FPS，尚未提供定位输入。V0.5.2 仅按用户请求增加坐标刻度/输入与帧率控制，上述原图、几何、对齐及无尺寸辅助线/图例继续保留。
