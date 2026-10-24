# PortCanvas Demo V0.5.0 验收记录（2026-10-24）

分支 `feature/v050-arena-camera-metrics`，基线 `main 9533d93`。测试使用独立客户端资料目录、会话命名空间和远端视频通道；未操作电机/GPIO/串口，未改业务程序或SDK。

## 功能与证据

| 检查 | 结果 | 证据 |
| --- | --- | --- |
| Rust核心 | 38项通过，含帧分包/并包、帧界限及既有连接/传输/日志/资料保护 | `05-测试与验证/local/v050/core-final.txt` |
| 前端与发布构建 | npm build、TypeScript检查、Tauri Windows Release成功 | `local/v050/release-final-corrected.txt`（本地验证目录） |
| 板端协作 | T027完成、15条检查、0文件修改；宿主SSH另行确认video0采集、video1元数据以及真实网卡 | `local/board/tasks/T027-v050-camera-metrics-readiness-20261024/returned/result.md`、`local/v050/host-probe.json` |
| 实机基础集成 | 9项通过：默认关闭/真实指标、真实JPEG、关闭释放、反复开关、切页释放、语言切换保持流、断连清空、旧ID隔离、无效设备拒绝 | `local/v050/integration-result.json` |
| 生命周期 | 7项通过：最小化释放、恢复保持关闭、迟到失败不覆盖新流、无消费自动停止、正常退出释放、重启默认关闭、仅测试客户端崩溃后释放 | `local/v050/lifecycle-result.json` |
| 静默断链看门狗 | 无心跳15.281秒退出、77帧、exit0、无stderr | `local/v050/lease-probe-result.json` |
| 画面内存边界 | 真实流 Blob URL 同时最多2个，关闭后0个；未见Renderer异常 | `local/v050/layout-result.json` |
| 比例与布局 | 3.8m内边界、2.4m台面、700mm边距、20mm墙厚；无模拟实体；1360×880、1000×680、中英文与展开终端检查 | `local/v050/layout-result.json`、`.impeccable/review/v050/` |

`local/`、摄像头截图和个人资料均忽略，不随 Git 或发布包分发。记录中的本地相对路径均位于 `05-测试与验证/`，除明确给出的 `.impeccable/` 外。

## 已发现并修复

- OpenCV read/sleep在实机出现采集超时，改为持续grab、按需retrieve/编码；stdin监控由带锁缓冲读取改为os.read，避免解释器退出争锁。
- Windows WebView在原生窗口最小化后仍报告页面可见，补原生最小化检查；实测释放251ms（单次，不视为性能保证）。
- 过期错误停止回执可能覆盖新视频状态，加入意图与流ID复核。
- 图片改用有界Blob URL并及时撤销；显示贴图缩至1024px，原图保持不变。
- 更新测试中的“未来版本”硬编码在升到0.5.0后失效，夹具使用当前版本patch+1。

## 验收边界

真实视频已验证，尚无实车定位/敌车/能量块输入；页面不提供假数据。8fps为配置上限，不是实测最低帧率承诺。未进行长时间比赛、全平台摄像头兼容或整机性能提升基准测试。摄像头关闭后无本软件视频采集进程；赛台页的轻量性能读取仍按需进行。

## 界面审查与最终交付检查

独立审查最终 disposition 为 ship，remaining clear：

| 发现 | 最终结果 |
| --- | --- |
| F1 最小窗口摄像头状态被挤出 | resolved，中英文状态均完整 |
| F2 展开终端内存单位截断 | resolved，允许完整数值与单位换行 |
| F3 英文漏译内存 | resolved，指标与图例均为Memory |
| F4 产品/设计文档缺赛台 | resolved，更新PRODUCT/DESIGN并补局部arena brief |

技能安装缺agents目录，独立审查者/文档者采用技能附带degraded角色说明，在新子代理上下文完成。检测器只运行一次，缺HTML解析模块降级为正则，不视为完整可访问性或对比度检查。动态隐藏img无静态src及缺值横线为有意状态，不填入假内容。

最终Release构建、布局断言和真实流检查通过。已安装V0.5.0，devices、settings、terminals、log-settings四文件升级前后字节保持；桌面快捷方式指向0.5.0图标。正式GUI用原DPAPI凭据连接成功，赛台显示真实指标且摄像头默认关闭。证据：local/v050/installed-result.json、installed-ui-result.json。

便携副本使用自身data/logs，空设备列表、默认浅蓝/中文、摄像头关闭且离线禁用。证据portable-result.json。同时打开两份同名应用时，原生截图提供器返回与可访问性/CDP不一致的重叠画面，未将此截图作为便携证据；采用明确端口9501的API、DOM与文件结果验证，并关闭测试副本。

发布包8个文件，ZIP自检通过，无个人data/、凭据、相机照片或测试资料。便携EXE与安装版仅官方UNK/NSS包类型标记不同；ICO含16/24/32/48/64/256方形尺寸。证据local/v050/package-result.json。

PR #15已按功能头 1b1435d75dbc605805030f5e4d66559061aeea70 合入main，合并提交 e1d186d68d923f56ad9b6ba3caf6b5e7781040c1。已关联聊天，并提交Codex审查面板（返回queued）。Code Review后端读取同一功能头，checks/statuses/annotations均为空；未将空CI视为独立代码审查通过。本地main已同步，后续提交仅同步文档与发布记录。
