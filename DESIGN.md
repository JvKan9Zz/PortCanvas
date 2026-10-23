---
name: Wheel Combat Studio
description: 面向 Windows 的中文设备连接、远程桌面、文件传输与持久终端工作区
colors:
  primary: "#166c54"
  primary-hover: "#11543f"
  focus: "#259c7a"
  selection-surface: "#e5eee9"
  navigation-active: "#dcece4"
  ink: "#23313a"
  muted: "#596c78"
  placeholder: "#65737e"
  list-note: "#5e707b"
  caption: "#596d79"
  status-text: "#5f707c"
  workspace: "#eef1f3"
  surface: "#fff"
  chrome: "#fafbfc"
  panel: "#f7f9fa"
  line: "#dce2e5"
  rail: "#202e38"
  rail-label: "#bac6ce"
  desktop: "#17222b"
  desktop-ink: "#e4ecf0"
  desktop-muted: "#9fb0bc"
  pending: "#b8791f"
  error: "#b4473d"
  danger: "#a84a40"
typography:
  headline:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "25px"
    fontWeight: 600
    lineHeight: 1.35
    letterSpacing: "-0.025em"
  title:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "17px"
    fontWeight: 600
  body:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "14px"
    fontWeight: 400
  label:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "12px"
    fontWeight: 500
  input:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "13px"
    fontWeight: 400
  helper:
    fontFamily: "Plex, 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif"
    fontSize: "11px"
    fontWeight: 400
rounded:
  field: "4px"
  button: "5px"
  toolbar: "6px"
  navigation: "7px"
  container: "8px"
spacing:
  compact: "8px"
  control: "16px"
  field-gap: "20px"
  editor: "30px"
  page: "32px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.surface}"
    rounded: "{rounded.button}"
    padding: "0 16px"
    height: "39px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "#354550"
    rounded: "{rounded.button}"
    padding: "0 16px"
    height: "39px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "#263842"
    typography: "{typography.input}"
    rounded: "{rounded.field}"
    padding: "0 11px"
    height: "40px"
  navigation-item:
    backgroundColor: "transparent"
    textColor: "{colors.rail-label}"
    rounded: "{rounded.navigation}"
    height: "71px"
  navigation-item-active:
    backgroundColor: "{colors.navigation-active}"
    textColor: "#17543e"
  device-workspace:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.container}"
  device-item-active:
    backgroundColor: "{colors.selection-surface}"
    textColor: "#1e533d"
    rounded: "{rounded.button}"
    padding: "16px 12px"
  tool-button:
    backgroundColor: "transparent"
    textColor: "#596c79"
    rounded: "{rounded.field}"
    padding: "0 8px"
    height: "30px"
  desktop-stage:
    backgroundColor: "{colors.desktop}"
  file-pane:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.toolbar}"
  file-path-input:
    backgroundColor: "#f9fbfc"
    textColor: "#263842"
    rounded: "{rounded.field}"
    padding: "0 11px"
    height: "34px"
  transfer-status:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.toolbar}"
    padding: "12px 16px"
  folder-dialog:
    textColor: "{colors.ink}"
    rounded: "{rounded.navigation}"
    padding: "25px"
    width: "410px"
---

# Design System: Wheel Combat Studio

## Overview

**Creative North Star: "设备操作工作台"**

这是中文 Windows 操作工具的现有视觉系统：灰白工作区承载设备信息、表单和双栏文件列表，深蓝灰固定侧栏稳定定位，绿色突出主要操作和选中状态。信息密度适中，文字、边框和分区承担层级，远程桌面画面和文件操作各自获得主要空间。

该名称概括已经建成的方向，不是新增品牌提案。用户明确指定固定导航与页面布局，并选择直接编码、优先功能；继续开发时沿用这一约定。Demo V0.1 的文件与擂台均为后续版本入口；V0.2 已将文件页实现为本地电脑与树莓派之间的 SFTP 浏览和传输界面，擂台仍为后续入口。连接与传输反馈来自实际状态，不为界面填充虚构内容。

V0.1 视觉记录于 2026-10-22 从 `03-实现与脚本/studio/ui/style.css`、`ui/index.html`、`src/renderer.js`、`src/main.cjs` 提取，并参照根目录 `PRODUCT.md` 与 `02-方案与设计/demo-v0.1-范围与实现.md`。该阶段目视参考为 `05-测试与验证/local/screenshots/` 中的 `devices-final.png`、`devices-small.png`、`desktop-offline.png`；它们是已存离线渲染，不能作为实时桌面、输入或设备连接验收。

V0.2 文件页在同日按 `02-方案与设计/demo-v0.2-文件传输方案.md`、`src/files-page.js`、`ui/files.css`、`ui/index.html` 与 `src/main.cjs` 合并记录；源码相对上述 studio 目录。目视参考为同一截图目录中的 `v02-release-upload.png`、`v02-release-files-small.png`、`v02-release-progress.png`、`v02-release-canceled.png`。本次仅核对已存源码与截图，未启动应用；CSS 组件示例不等于原生窗口实测。`PRODUCT.md` 中 V0.1 的范围描述保留为历史背景，当前文件页以 V0.2 实现为准。

**Key Characteristics:**

- 固定左侧导航与稳定的页头、状态栏。
- 白色设备编辑区和双栏文件区、浅灰背景与深色桌面画布。
- 绿色主要动作，小圆角、细边框、克制的状态变化。
- 中文操作文案、图标配文字、真实状态反馈。

## Colors

以冷灰与蓝灰建立工作环境，绿色引导可执行操作；完整色值以顶部 frontmatter 为准，组件边界和状态细节见 sidecar。

### Primary

- **操作绿**（`primary`）：连接设备、连接桌面、上传等主要动作；已连接状态点、传输进度及输入光标使用同一色。
- **深操作绿**（`primary-hover`）：主要按钮悬停反馈。
- **焦点绿**（`focus`）：键盘焦点外框，与选中底色分工明确。
- **浅绿选择层**（`selection-surface`、`navigation-active`）：设备项和当前导航的静态选中底色，保持深色文字可读。

### Neutral

- **蓝灰墨色**（`ink`）与 **次级蓝灰**（`muted`）：标题、正文和辅助信息。
- **占位蓝灰**（`placeholder`）、**列表注释灰**（`list-note`）、**画布说明灰**（`caption`）与 **底栏文字灰**（`status-text`）：分别用于输入提示、保存说明、画布说明/后续版本标签与状态栏文字，避免在小字号上使用过浅文字。
- **工作区灰**（`workspace`）：页内容外层背景。
- **编辑白**（`surface`）、**栏位近白**（`chrome`）、**列表浅灰**（`panel`）：编辑区域、页头/状态栏和设备列表，依靠轻微明度差区分职责。
- **分隔浅灰**（`line`）：容器边界与行间分隔。
- **侧栏深蓝灰**（`rail`）与 **侧栏文字灰**（`rail-label`）：导航底板与未选中入口。
- **画布深蓝黑**（`desktop`）、**画布亮文字**（`desktop-ink`）、**画布次级文字**（`desktop-muted`）：远程画布及离线提示。

连接中的琥珀色（`pending`）、连接错误红（`error`）及移除操作红（`danger`）只表达各自语义。它们不是额外品牌强调色。

**The State Truth Rule.** 状态文字、状态点与可用动作必须对应实际设备或桌面状态；绿色不能单独充当连接成功的证据。

## Typography

拉丁字体使用本地 IBM Plex Sans，CSS 家族名为 `Plex`，由 `plex-latin.woff2` 加载；中文依次回退到 Microsoft YaHei UI、Microsoft YaHei，最后为系统 sans-serif。没有单独的宣传展示字体或代码等宽字体。

### Hierarchy

- **Headline**：设备页主标题；桌面页与文件页主标题使用较紧凑尺寸（23px），后续版本占位页为（24px）。
- **Title**：编辑区标题；桌面离线提示标题为（20px、500）。
- **Body**：基础文字；段落使用较舒展行高（1.7），帮助短说明阅读。
- **Label / Input**：表单标签与输入内容分别使用 frontmatter 的角色，辅助提示更小、更淡。
- **Helper**：设备地址、导航文字及底部辅助信息；应用底栏为（10px）。

品牌字标使用轻微字距（0.09em）；导航文字为（11px、0.08em）。端口与设备地址采用等宽数字特性（`tabular-nums`），不引入另一套字体。设备名称和长错误信息允许换行，不以缩小字号隐藏内容。

文件面板标题为（14px），列表与路径内容为（11px），面板底部项目/选择计数与当前文件为（10px）。传输标题为（12px、600），数量、速度和说明为（11px）。列表大小与日期、传输数量采用等宽数字特性；长文件名和当前文件单行省略，名称与当前文件保留完整 title，错误说明允许换行。

## Layout

用户指定的布局是本产品约束。固定左侧栏宽（84px），应用内容以等量左边距避让；侧栏从上到下为 **设备、桌面、文件、擂台**。页头高（65px），底部状态栏高（29px），中间内容区占剩余高度。默认 Electron 外窗（1360×880），最小外窗（1000×680）；这些是源码配置，不等于网页视口大小。

设备页四周默认留白（32px）。单一白色工作区采用左侧设备列表与右侧连接编辑器，列表宽（250px），右侧为可收缩列。表单为两列，设备名称跨两列；行列间距默认（20px、22px）。高级桌面设置以折叠区域组织。连接结果放在表单下方，以分隔线隔开。

桌面页采用纵向弹性布局：页面标题与操作、工具条（43px）、剩余空间的大画布、画布说明栏（33px）。画布自身可滚动，适配与原始尺寸仅改变桌面显示模式。离线内容在画布内居中，保持操作入口与状态说明可见。

V0.2 文件页采用纵向弹性布局，默认留白为（25px 28px 18px），区块间距（15px）。页标题与 SFTP 状态在上，上传/下载和限速位于同一操作行；中间为等宽的本地/远端两列（`minmax(0,1fr)`，间距15px，最小高度180px），下方常驻一块传输状态区。每栏自上而下为标题（52px）、目录工具、路径、独立滚动表格与项目/选择计数。表头固定在各自滚动区顶部，不让长列表推走传输状态。

宽度断点（1120px）以内，设备页留白收为（25px），列表宽度收为（220px），编辑区留白收为（24px）；连接结果允许换行，隐藏编辑标题旁的次要状态标签。高度（760px）以内压缩页顶、标题和表单间距。短窗口允许设备页纵向滚动，页头、导航和底栏保留。当前没有移动端导航或把设备表单改为单列的实现。

文件页在宽度（1120px）以内保持双栏，左右留白收为（24px），区块及栏间距收为（12px）；隐藏修改时间列，大小列由（85px）收为（80px），为名称和路径保留空间。高度（760px）以内，页上下留白收为（19px、13px），区块间距（10px），栏标题收为（42px），目录工具、路径和传输区压缩垂直留白。没有文件双栏堆叠或新增移动布局。

**The Stable Rail Rule.** 调整窗口时保留侧栏的宽度、入口顺序和图标配文字形式，不擅自更换为顶部导航。

## Elevation & Depth

整体以平面层级为主：背景明度、细边框和分隔线划分区域，设备工作区、文件栏、传输状态、按钮、输入与桌面工具条均不使用常驻投影。临时 toast 使用投影（`0 6px 20px #10273226`）帮助反馈浮于内容之上（z-index 10）。V0.2 新建文件夹 dialog 使用投影（`0 12px 40px #132a352e`）与半透明遮罩（`#142f3b55`）标示模态层；不扩展为多层卡片或玻璃面板。

界面状态过渡主要为短时颜色变化（0.15s），没有页面入场或画布装饰动画。连接中状态点使用（1.2s、ease-in-out）的透明度脉冲；`prefers-reduced-motion: reduce` 关闭动画和过渡。

## Shapes

细边框、小圆角、直线布局。输入、提示标签和工具按钮使用最小圆角；操作按钮与设备项略大；导航、标志容器和工作区递增一档。精确圆角见 frontmatter。桌面工具条只将上方两角圆化，画布与其贴合。状态点及连接结果图标为圆形，这是语义标记，不是通用胶囊风格。

文件栏和传输状态使用与工具条同档的小圆角，新建文件夹 dialog 使用导航同档圆角；进度条为细线（5px 高、3px 圆角）。文件表格保持直线行分隔，不把每个文件做成独立卡片。

图标为线性 SVG，常规（20px、1.65px 描边），导航（23px），操作按钮（17px），桌面工具（16px）。保持圆端点和圆连接，不用照片或插画替代操作图标。

## Components

### Buttons

主要按钮为绿色实底，次要按钮为白底灰描边，文字操作用于移除配置。常规按钮固定高度与水平内边距见 frontmatter；文字和图标间隔（9px）。悬停改变底色，次要按钮同时加深边框。禁用按钮使用不透明度（0.45）并取消操作型光标，不以文字颜色伪装可点击。

### Inputs / Fields

白底、细蓝灰描边，标签在上、输入在下（8px 间隔）。悬停加深边框，输入焦点边框转为操作绿；键盘焦点额外显示（2px）焦点绿外框，外偏移（4px）。禁用输入改为浅灰底。密码标签明确写“不保存”；连接或连接中时，设备配置输入与保存、添加、移除入口禁用。

### Navigation

深蓝灰垂直侧栏；入口居中排列图标与中文名称，间隔（7px），导航项之间（9px）。普通悬停以较亮蓝灰反馈，当前设备/桌面/文件入口为浅绿底与深绿文字。V0.2 只有擂台入口保留较淡文字，进入后以灰蓝选中底显示后续页面；V0.1 文件入口曾采用同样的后续样式。保留文字名称、title 与当前页语义，不只依赖图标或颜色。

### Device List / Connection Workspace

单一带边界的工作区容纳浅灰设备列表和白色编辑器。列表项显示设备名称及用户/地址/端口，选中项加浅绿底与细绿灰边界；标题可以折行。已保存设备数为小型淡灰计数标签。右侧编辑器使用设备图标、标题、可选状态标签、表单、动作区和结果区，避免再嵌套装饰卡片。

### Status / Feedback

页头连接徽标、编辑区结果、画布下方状态和底部状态栏服务不同位置的读数。连接结果与全局状态提供文本反馈；结果区使用 `aria-live="polite"`，底部使用状态角色，toast 使用提醒角色。toast 右下固定显示（最多430px），保留多行文字，当前自动隐藏时间（6000ms）。

### Desktop Toolbar / Canvas

白色紧凑工具条放在深色画布上方，左侧适配/1:1，右侧只读、Ctrl+Alt+Del、全屏。选中工具使用浅绿底；只读入口保留 `aria-pressed`。桌面断开、连接中、错误各有对应文字；未连接时显示画布内说明和下一步入口。只有收到桌面实际连接事件才隐藏离线层，禁止填入示意桌面冒充在线画面。

### File Panes / Directory Controls

本地电脑在左、树莓派在右，两栏均使用白底细边框容器。栏内保留上一级、主目录、刷新、新建文件夹与路径输入；本地额外有“选择目录”，远端未连接时有前往设备页的“连接设备”。路径输入为紧凑高度，末端箭头或 Enter 提交路径。目录双击或 Enter 打开；文件行单击单选，复选框、Ctrl/Meta 单击和 Space 可增减选择，表头复选框支持全选与部分选中。切换目录后清除本栏选择。

表格按选择框、名称、大小、修改时间排列，表头浅灰、行底细分隔；悬停轻绿灰、选中浅绿。选择框为（14px），名称图标为（16px），行单元格高度（35px）、内边距（6px 9px）。键盘焦点使用内收外框（2px、offset -2px）。文件栏底部以文本显示项目总数、已选数量、读取中或读取错误；空目录与远端未连接各有真实说明。

上传为绿色主要按钮，下载为描边次按钮；文件页操作行按钮紧凑为（36px 高、12px 字号）。传输方向文案明确写出目标。限速选择提供不限速、1/5/10 MiB/s。远端离线、目录读取中、没有源选择或目标尚未加载时禁用对应传输入口；传输期间禁用目录、选择、路径、上传/下载和限速，保留取消入口。不要仅以绿色 SFTP 点推断文件已传完。

### Transfer Status / Confirmation

单一传输状态区放在双栏下方，顶部为阶段标题、字节/文件数量及可用时的速度、取消按钮，其下依次为细进度条、说明与当前文件/目标路径。阶段包括准备、检查、确认覆盖、上传/下载、正在取消、完成、已取消、失败；已取消保留当前进度和完成数量，不显示为完成。失败说明采用红色文字（`#a54438`）并保留可换行的具体原因。数量和状态由传输事件更新，只有完成状态将进度置为100%。

覆盖确认由 Electron `dialog.showMessageBox` 原生问题窗口承担，显示同名文件数量和目标路径，按钮为“取消传输”“覆盖这些文件”，默认及取消动作均为取消传输。它没有自绘 CSS 视觉规则，不用 sidecar 中的 HTML 模态示例冒充原生确认窗。取消和失败的说明遵循已建成传输行为：当前未完成文件不发布，之前完成的其他文件保留。

### New Folder Dialog

本地或远端新建文件夹使用 HTML `dialog.showModal()`，标题明确所属端，提供名称输入、就地错误与右对齐的取消/创建按钮。宽度（410px），最大宽度（`calc(100vw - 50px)`），留白（25px）；标题、按钮沿用既有字号和形状。打开后聚焦名称，提交期间禁用创建按钮，失败留在 dialog 中显示错误，成功后关闭并刷新对应目录。该 dialog 与原生覆盖确认是不同组件。

### Future Entries

当前仅擂台页面沿用背景、字体和线性图标，以居中的标题、说明和小标签表达等待视觉定位与场景数据接入。V0.1 的文件占位页为历史范围；V0.2 已由上述双栏文件页替代。文件页的加入不扩展为实时坐标、视频或车辆动作。

## Do's and Don'ts

### Do:

- **Do** 保留固定侧栏与设备、桌面、文件、擂台的顺序。
- **Do** 以灰白表单区和深色大画布延续现有 Windows 工具布局。
- **Do** 使用本地 Plex 拉丁字体与微软雅黑中文回退，保持中文标签可读。
- **Do** 同时用文字与状态点说明真实连接状态，并让动作可用性与状态一致。
- **Do** 保留键盘焦点、禁用反馈、长文本换行和减少动态偏好。
- **Do** 用已有断点压缩留白并允许设备页滚动。
- **Do** 保留文件双栏、各自目录与选择计数，并在窄视口隐藏修改时间列。
- **Do** 用传输阶段、数量、当前文件和取消状态共同反馈实际进度，覆盖前保留原生确认。

### Don't:

- **Don't** 将固定侧栏改成营销页导航、移动端抽屉或重新排序入口。
- **Don't** 加入宣传大标题、品牌插画、装饰渐变或多层投影卡片。
- **Don't** 将存档离线截图写成实时桌面或硬件功能已验收。
- **Don't** 将 V0.1 的文件占位说明当成 V0.2 当前界面，或把擂台后续入口描述为已实现业务。
- **Don't** 为视觉演示生成在线设备、擂台数据、文件传输或机器人状态。
- **Don't** 将 HTML dialog 或 CSS 示例写成原生覆盖确认窗口已经实测。


## V0.3 右侧持久终端栏

2026-10-23 最终布局：左侧四个导航入口保持。桌面、文件和擂台页面右上角的“终端”控制右侧非模态栏，设备页隐藏；收起和切页保留终端状态。展开后主内容与终端并排，不遮挡操作区；侧栏宽度 clamp(360px,40%,580px)，窄布局隐藏文件表修改时间列。1000×680 CSS 视口的并排边界、输入区与无横向溢出已检查。

终端沿用现有深色桌面表面 #17222b，使用 Cascadia Mono / Consolas 14px 字体。连接状态、恢复/断开、清屏、会话标识和输入提示分层显示；焦点有明确描边，输出通过 xterm 屏幕阅读支持呈现。离线时禁止继续输入，恢复后显示远端已有会话内容。

擂台未来左下角调整为真实接口返回值区。该区与右侧指令终端分工；接口、单位与刷新节奏后续以实际板端资料和接线核实，不填造实时值。

## V0.3.5 本地日志页

导航在文件与擂台之间增加“日志”。页面沿用文件页的 25px/28px 留白、白色细边框面板和深色输出文本区；项目/日期/状态筛选位于列表上方，清理按钮显示已选数量，活跃记录的复选框禁用。列表行显示命令、项目、时间、大小及带文字的结果状态。详情显示退出码、时间、目录、完整性与本地路径。

内容区域低于 760px 时，列表和详情上下排列，外层可滚动；1000×680 CSS 视口与右侧终端并排检查无横向溢出。空详情显式使用 hidden，不被 flex 样式覆盖。清理使用原生取消/删除确认。终端栏新增“日志项目”输入，留空表示自动识别。


## Demo V0.4.9 主题补充（2026-10-24）

当前PortCanvas/Tauri以studio-tauri/ui/themes.css为色彩权威。上述绿色叙述保留为历史基线；默认改为Logo对应的浅蓝按钮#9bd4f3、深文字#163d54、选中底#e6f2fa、选中文字#224f69、焦点#287aa6。保持现有黑白灰表面、字体、留白、布局和Logo。

设置提供浅蓝、绿色、灰色，映射同一套动作/焦点/选区/边界/终端选区语义变量。用户内容与远端程序的ANSI输出不随主题改写，错误/警告/成功有独立语义。主题只换色、不重建连接，存入已有settings.json；旧文件缺字段时默认blue。
