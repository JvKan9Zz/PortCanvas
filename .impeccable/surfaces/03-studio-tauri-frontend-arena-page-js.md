---
version: 1
slug: "03-studio-tauri-frontend-arena-page-js"
primary_target: "03-实现与脚本/studio-tauri/frontend/arena-page.js"
related_targets: ["03-实现与脚本/studio-tauri/ui/index.html","03-实现与脚本/studio-tauri/ui/arena.css","03-实现与脚本/studio-tauri/frontend/en.json"]
---

# V0.5.0 赛台表面

- 模式：Operate；在既有 PortCanvas 工作区内操作。
- 范围：`frontend/arena-page.js` 驱动的赛台页；关联 `ui/index.html`、`ui/arena.css`、`frontend/en.json`，路径均相对 `03-实现与脚本/studio-tauri`。
- 使用者与任务：开发者连接真实 Linux 设备，按需手动查看摄像头，并将真实系统性能与赛台几何同时观察。
- 主动作：连接设备后选择视频设备并开启摄像头；关闭会释放本次采集。默认关闭，切页、最小化、断连、退出后停止，返回后需手动开启。
- 构图：常规宽度左侧约三分之一为摄像头在上、性能在下，右侧约三分之二为赛台；在窄主内容区重排并滚动，保留完整状态、数值和单位。
- 方向：用户明确构图的代码实现，沿用灰白表面、浅蓝默认主题、Plex/中文回退、线性图标与带边框紧凑按钮；未建立新视觉系统。
- 核心内容：围墙内 3.8 × 3.8 m，中央台面 2.4 × 2.4 m，台下宽 0.70 m，用户提供的 1024 px 台面图案与红色围墙。真实 Linux 指标及最近 60 秒 CPU/内存曲线来自 SSH；摄像头 JPEG 输出至多 640 × 480、8 帧/秒，不录制。
- 记忆点：手动开启的真实画面与大幅等比例赛台并置；信息密度服从观察任务。
- 约束：缺值显示“—”，错误显示原因；定位、车辆、敌车、能量块、比赛接口返回值未接入，实体层保持为空。
- 未决项：上述业务实体的真实接口与坐标接入另行确定；本简报不承诺相关功能或实车验收。
