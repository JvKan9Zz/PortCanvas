# PortCanvas Demo V0.5.1 验收记录（2026-10-24）

基线main 03eb796，分支fix/v051-arena-surface-layout。范围为指定台面图片、尺寸标注移除、赛台放大、出发区位置及版本号。用户追加黄色上边/蓝色下边与中央台面对齐后，已合并进本版。摄像头、SSH与指标服务实现未改，本轮不重复执行或声称通过V0.5.0的板端/生命周期全量测试。

| 检查 | 实际结果 |
| --- | --- |
| 核心测试 | Rust Release 38/38通过 |
| 构建 | npm build、TypeScript与Tauri Windows Release通过，最终release-aligned.txt |
| 原图 | 工作区6803×6803原件按字节复制，SHA-256 f05a8e3defbd69b99ed365300c561b5ae9e3ad454adc53ca8af030a69e94a267 |
| 显示文件 | 1536×1536 PNG，187982字节，ICC 3144字节；重新打开后像素与等比缩小结果完全相同 |
| 客户端资源 | Network资源响应SHA-256与磁盘PNG一致：baff57a247a5776cd4f1caf6f74b0b0c6bc9cd0f9ee3098160cfc8e32b33d39f；实际图像解码1536×1536 |
| 移除标注 | SVG内无尺寸text节点、无尺寸辅助线、无底部尺寸图例 |
| 比例与放大 | 保留3800/2400坐标；1360×880下赛台边长由约539.8px变为680.8px，增加约26.1% |
| 出发区 | 黄(0,700,500,400)，蓝(3300,2700,500,400)；四个视图黄上边/蓝下边与台面对应边误差均小于0.1 CSS px |
| 布局 | 1360×880、1000×650内容视口（含原生最小窗口标题栏差额）、英文、展开终端：正方形比例/无横向溢出/摄像头状态完整；短窗口允许页面滚动 |
| 未接入状态 | 无模拟实体，离线指标为空值，摄像头保持关闭并禁用 |
| Renderer | 异常0 |

本轮使用Impeccable polish局部修正流程，在既有系统内核对四张实际Release截图；没有创建新品牌或新页面概念。检测器只运行一次，缺解析依赖降级为正则；动态隐藏img无静态src和缺值横线为既有有意状态，不据正则报告宣称对比度完整验收。

机器证据均在05-测试与验证/local/v051/：asset-result.json、ui-result.json、core-tests.txt、release-aligned.txt、detector.json/.stderr。截图在.impeccable/review/v051/，这些测试目录忽略、不随发布包分发。安装、便携和PR结果见下文。

正式升级到0.5.1后，devices/settings/terminals/log-settings四文件字节保持，桌面快捷方式指向0.5.1 ICO；未强制终止旧进程。证据local/v051/installed-result.json。

干净便携副本以自身data/logs运行，设备列表为空；1536px图片正常解码、黄y700/蓝y2700正确，摄像头关闭。ZIP8文件、无个人资料；安装/便携EXE仅官方UNK/NSS包类型标记不同。证据local/v051/portable-result.json、package-result.json、installed-ui-result.json。正式客户端已打开赛台页，保持离线和摄像头关闭。

PR #16按功能头1551c0b8dd5f414460a17abf22e32dde166c65f3合入main，合并提交51a3d64aab7dde2a9b37cd1a3197257314ef4cba；关联当前聊天并提交审查面板（queued）。Code Review后端同头checks/statuses/annotations均为空，未据空CI称外部审查通过。本地main已同步。后续仅同步文档/发布记录。
