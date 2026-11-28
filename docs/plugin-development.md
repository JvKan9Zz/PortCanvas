# PortCanvas 插件开发规范（v0.7.2 · declarative-v1）

PortCanvas 从 `v0.7.2` 起采用 **「纯原生声明式 UI（`ui.json`）+ 标准本地 Worker IPC（`pc-worker-v1`）+ 宿主共享 SSH 桥」** 的百宝工具箱架构。开发者与用户无需修改或重新编译 PortCanvas 主程序，即可开发、打包并通过插件页「上传插件 → 导入本地插件」离线安装任意类型的工具插件。

---

## 1. 插件包格式（`.pcplugin`）

`.pcplugin` 文件本质是一个标准的 **USTAR + Gzip 压缩归档（`.tar.gz`）**，根目录必须包含 `manifest.json`：

```text
my-tool-0.1.0.pcplugin
├── manifest.json          # 必填：插件清单与权限声明
├── ui.json                # declarative-v1 必填：原生界面声明
├── bin/                   # local-worker 可选：本地可执行程序（Rust/Go/C++/Python 等）
│   └── my-worker.exe
├── scripts/               # ssh 插件可选：远端控制器执行脚本
│   └── inspect.py
└── assets/                # 可选：图片等静态资源
    └── bg.png
```

### 归档安全边界（强制校验）
- 仅允许普通文件与目录，**禁止**符号链接（symlink）、硬链接与特殊设备文件。
- 所有路径必须是相对路径，禁止 `..`、绝对路径、重复（大小写不敏感）条目或 Windows 保留设备名（`CON`、`NUL`、`COM1` 等）。
- 单包体积与解压上限均为 **256 MiB**，条目数不超过 **2048**。
- 在线官方目录强制 **HTTPS + SHA-256 + Minisign 签名**；本地离线导入允许未签名自制插件，但安装前会弹出**权限与风险确认弹窗**，并在卡片上标注「本地」。

---

## 2. `manifest.json` 字段规范

```json
{
  "schema": "portcanvas.plugin/1",
  "id": "my-custom-tool",
  "name": "我的工具",
  "nameEn": "My Custom Tool",
  "version": "0.1.0",
  "summary": "一句话说明工具用途",
  "summaryEn": "One-line English summary",
  "category": "tools",
  "author": "YourName",
  "official": false,
  "minAppVersion": "0.7.2",
  "hostSurface": "declarative-v1",
  "platform": "windows-x86_64",
  "requiresDevice": false,
  "lifecycle": "foreground",
  "icon": "plugins",
  "ui": "ui.json",
  "provides": [],
  "runtime": {
    "kind": "local-worker",
    "entry": "bin/my-worker.exe",
    "args": ["--worker"],
    "frameChannel": false
  },
  "permissions": ["local.exec", "local.fs"]
}
```

### 关键字段说明

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `schema` | `string` | 固定为 `"portcanvas.plugin/1"` |
| `id` | `string` | 小写字母开头，仅含 `a-z`、`0-9`、`-`，长度 `<= 80` |
| `version` | `string` | SemVer（如 `"0.1.0"`），升级时必须 `>=` 已安装版本 |
| `minAppVersion` | `string` | 所需最低 PortCanvas 版本（声明式插件为 `"0.7.2"`） |
| `hostSurface` | `string` | 新插件固定填写 `"declarative-v1"` |
| `platform` | `string` | `"any"`（跨平台脚本）、`"windows-x86_64"` 或 `"darwin-universal"` |
| `requiresDevice` | `bool` | `false` 表示未连接 SSH 设备也可直接打开（如视频采集、下载器）；`true` 表示依赖已连接控制器 |
| `lifecycle` | `string` | `"foreground"`（切页/最小化时自动停止，节省资源）或 `"background"`（切页后后台常驻，卡片显示「运行中」状态点与摘要，退出/卸载时安全保存并停止） |
| `icon` | `string` | 可选内置图标：`"plugins"` / `"camera"` / `"download"` / `"arena"` / `"vision"` |
| `ui` | `string` | 声明式界面文件路径（通常为 `"ui.json"`） |
| `provides` | `string[]` | 可选宿主能力替换声明，例如 `["downloader"]` 表示安装启用后接管宿主内置下载引擎，停用或卸载后自动恢复内置 32 线程下载器 |
| `runtime` | `object` | 运行时配置：`"kind": "local-worker"`（需提供包内相对路径 `entry`、可选 `args` 与 `frameChannel`）或 `"kind": "ssh"` |
| `permissions` | `string[]` | 声明所需权限，离线导入确认弹窗会逐条向用户展示：`ssh.exec`、`camera.stream`、`pose.stream`、`vision.watch`、`local.exec`、`local.capture`、`local.network`、`local.fs` |

---

## 3. `ui.json` 声明式原生界面规范

宿主自动将 `ui.json` 渲染为 PortCanvas 原生 Slint 液态玻璃组件，无需编写任何 HTML/JS 或重新编译宿主。

### 顶层结构

```json
{
  "layout": "split",
  "primaryWidthRatio": 0.62,
  "headerBadge": {
    "statusKey": "status",
    "phaseKey": "statusPhase",
    "defaultText": "就绪",
    "defaultTextEn": "Ready"
  },
  "initialState": {
    "url": "",
    "threads": 128
  },
  "presets": {
    "hsvPreset": {
      "0": { "hLow": 0, "hHigh": 10 }
    }
  },
  "primary": [ /* 左侧或主区域区块列表 */ ],
  "secondary": [ /* split 模式右侧控制区区块列表 */ ]
}
```

- `layout`: `"split"`（左右双栏响应式，窄屏自动折叠）或 `"stack"`（单栏纵向布局）。
- `visibleWhen` / `disabledWhen` 条件对象：
  ```json
  { "key": "algo", "equals": 3 }
  { "key": "mode", "in": [1, 2] }
  { "key": "active", "truthy": true }
  ```

### 支持的 6 大通用区块（`BlockSpec`）

1. **`toolbar`**：顶部控制条，包含 `items`（下拉、分段、按钮列表）。
2. **`video-stage`**：低延迟原生视频舞台，自动接收 SSH JPEG 或本地 Worker 二进制帧流；支持 `fitModeKey`（`0=适配 / 1=填充 / 2=1:1`）、`rotationKey`（`0/90/180/270`）、`flipHKey`、`flipVKey`、`showSnapshot`（一键保存 PNG 快照）以及 `allowFullscreen`（开启远程桌面同款窗口内液态玻璃展开全屏，支持 `Esc`/双击还原）。当 `runtime.entry` 指向 `.py` 脚本时，宿主会自动调用本机 Python（`-u` 无缓冲模式）启动，方便快速原型开发（如 `plugins/_template/bin/worker.py`）。
3. **`arena-map`**：2D 坐标系平面地图画布，加载 `platformAssetKey` 底图并实时渲染位姿点与航向箭头。
4. **`kpi-grid`**：2/3 列实时指标网格，支持静态 `items` 或动态 `dynamicItemsKey`（状态中数组 `[{"label":"FPS","value":"60.0"}]`）。
5. **`control-card`**：参数配置卡片，可组合以下控件：
   - `segmented`（分段选择）
   - `select`（静态或 `optionsKey` 动态下拉，动态项格式 `[{"label":"...","value":"..."}]`）
   - `checkbox`（开关复选框）
   - `slider`（液态玻璃滑块，支持 `min`/`max`/`step`/`unit`）
   - `input`（单行输入框，支持回车 `submitAction`）
   - `dir-picker`（本地目录选择器，宿主弹出系统选目录弹窗后回传所选路径）
   - `button` / `button-row`（主按钮/普通/危险按钮，支持 `activeKey` 文案切换与 `busyKey`）
   - `code-snippet`（只读代码框 + 一键复制）
   - `notice`（提示/错误横幅）
6. **`task-list`**：动态任务/条目列表，绑定 `itemsKey`，每条结构如下：
   ```json
   {
     "id": "task-1",
     "title": "model.bin",
     "subtitle": "64.2 MB / 128 MB · 42.5 MB/s · 128 线程",
     "badge": "下载中",
     "badgeTone": "accent",
     "progress": 0.5,
     "showProgress": true,
     "actions": [
       { "id": "pause", "label": "暂停", "labelEn": "Pause" },
       { "id": "remove", "label": "删除", "labelEn": "Delete", "danger": true }
     ]
   }
   ```
   `badgeTone` 可选：`"muted"`、`"accent"`、`"ok"`、`"danger"`。

---

## 4. `pc-worker-v1` 本地 Worker 通信协议

当 `manifest.runtime.kind == "local-worker"` 时，宿主启动插件包内可执行文件，通过 **stdin / stdout 的逐行 JSON（NDJSON，UTF-8）** 双向通信；进程退出或主程序关闭时宿主会通过系统作业对象确保不残留孤儿进程。

### 4.1 宿主 → Worker（stdin 每行一条 JSON）

- **初始化握手**（启动后首条消息）：
  ```json
  {
    "type": "init",
    "pluginId": "my-custom-tool",
    "dataDir": "C:/Users/.../PortCanvas/plugin-data/my-custom-tool",
    "defaultDownloadDir": "C:/Users/.../Downloads",
    "language": "zh",
    "frameAddr": "127.0.0.1:54321",
    "frameToken": "64位十六进制一次性令牌"
  }
  ```
  > `dataDir` 为插件专属持久化目录，插件升级或卸载重装不会清除该目录。
- **控件状态变更**：
  ```json
  { "type": "set_state", "key": "threads", "value": 128 }
  ```
- **按钮 / 行操作 / 目录选择触发**：
  ```json
  { "type": "action", "action": "pause", "itemId": "task-1", "payload": null }
  ```
- **前后台可见性切换**：
  ```json
  { "type": "visibility", "visible": false }
  ```
- **优雅关闭**（Worker 收到后应立即保存持久化状态并退出，超时 2 秒宿主将强制终止）：
  ```json
  { "type": "shutdown" }
  ```

### 4.2 Worker → 宿主（stdout 每行一条 JSON）

- **增量状态更新**（合并至插件界面状态，驱动所有控件、KPI、`task-list` 与后台卡片摘要）：
  ```json
  {
    "type": "state_patch",
    "patch": {
      "status": "运行中",
      "statusPhase": 2,
      "backgroundActive": true,
      "backgroundSummary": "2 个任务 · 58.4 MB/s"
    }
  }
  ```
  > `statusPhase`：`0=待机(灰)`、`1=进行中(蓝)`、`2=正常/活跃(绿)`、`3=异常(红)`。
- **右下角轻提示**：`{ "type": "notify", "message": "已完成" }`
- **错误提示条**：`{ "type": "error", "message": "无法打开设备" }`
- **复制文本到剪贴板**：`{ "type": "copy_text", "text": "..." }`
- **打开本地文件或目录**：`{ "type": "open_path", "path": "D:/Downloads" }`（仅允许已存在的本机绝对路径）

### 4.3 低延迟二进制视频帧通道（`frameChannel: true`）

1. Worker 连接 `init.frameAddr`（建议开启 `TCP_NODELAY`），首先原样发送 `init.frameToken` 的 UTF-8 字节完成身份认证。
2. 随后循环发送固定 20 字节小端帧头 + 载荷：
   - `[0..4]`：魔数 `b"PCFR"`
   - `[4..8]`：`width`（`u32 LE`）
   - `[8..12]`：`height`（`u32 LE`）
   - `[12]`：`format`（`1 = RGBA8`，`2 = JPEG`）
   - `[13..16]`：保留字节 `0`
   - `[16..20]`：`payload_len`（`u32 LE`，最大 32 MiB；`RGBA8` 必须等于 `width * height * 4`）
   - `[20..20+payload_len]`：像素或 JPEG 载荷
3. 宿主端采用 **最新帧覆盖（latest-frame-wins）** 策略，确保 1080p60 零队列堆积与极低渲染延迟。

### 4.4 宿主内置下载器接管协议（`provides: ["downloader"]`）

若插件在 `manifest.json` 中声明 `"provides": ["downloader"]`，用户安装并启用后，PortCanvas 的内置下载任务（插件下载、应用更新、板端离线组件包）将自动调用该 Worker 的单次下载模式，卸载或停用后立即回退为宿主内置 32 线程下载器：

```bash
<entry> --fetch --url <URL> --output <TEMP_FILE> --threads 128 --max-bytes 268435456 [--https-only] [--no-proxy] [--no-redirect]
```

Worker 通过 stdout 输出进度行：
```json
{ "type": "progress", "delta": 65536, "total": 10485760, "connections": 128 }
```
成功写入完整文件后以退出码 `0` 结束；失败则向 stderr 输出中文错误原因并以非零退出码结束。

---

## 5. 打包与本地调试

仓库内提供开箱即用的脚手架 `plugins/_template/`，可直接复制修改并运行一键打包脚本：

```bash
python plugins/_template/pack.py ./my-plugin ./my-plugin-0.1.0.pcplugin
```

打包完成后，在 PortCanvas「插件」页点击右上角 **「上传插件」→「导入本地插件」** 选择生成的 `.pcplugin`，核对权限弹窗后确认安装即可立即使用。
