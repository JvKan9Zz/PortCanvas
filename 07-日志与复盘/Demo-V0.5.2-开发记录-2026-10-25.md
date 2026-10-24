# Demo V0.5.2 开发与检修记录（2026-10-25）

遵循当前三份全局文档、项目规则与Stop That Shit；Impeccable在既有Operate世界内扩展赛台。用户明确当前无定位程序，仅实现坐标/接入/未接入状态。厂商SDK、传感器、比赛程序、运动控制、公共服务均未改动。

T028板端Codex任务完成15项命令、无文件改动；沙箱看不到/dev，不能据此断言主机无摄像头。任务结束后为避免Agent负载干扰，电脑通过既有SSH执行确定性采样，核实实际V4L2 MJPG/YUYV与FFmpeg能力。未提取凭据、未安装板端依赖。board_task.py没有--help入口，首次调用因参数路由失败后读取源码再使用正确入口。

原链路OpenCV逐帧解码/重编码与JSON载帧成本较高。本轮尝试二进制IPC、逐帧Image及Canvas/ImageBitmap；真实完整进程树测量显示电脑缓存/CPU成本仍偏高，未将这些尝试作为最终实现。最终用已有FFmpeg复制原生MJPEG，经SSH到Rust最新帧缓冲，再由仅127.0.0.1的临时令牌通道供WebView2原生连续显示；IPC只传状态。实测板端CPU下降、交付帧率提高，流量和PC私有内存/CPU上升，文档明确取舍。

实机UVC帧在EOI后有零填充，初版严格尾标记检查拒绝；有限单帧探针仅记录JPEG标记位置，不保存照片。去除尾部零填充并用FFmpeg mjpeg2jpeg位流过滤器补标准头（不解码重编码）后正常显示。中间自定义camera_frame命令曾缺Tauri ACL；最终架构已删除该命令/权限，保留既有studio_call校验。生产CSP只给img-src加入127.0.0.1，不放宽脚本或连接来源。

原点采用围墙内左下角，米制输入转换为SVG毫米且Y反向；真实输入新鲜度按板端时间核验，客户端另有2秒失效清除。协议测试只写独立/dev/shm/portcanvas-v052-test-pose.json，使用身份/内容核对清理，不改生产源。正常界面无合成位置。

最小化实测发现摄像头能停止但定位观察器仍存活：WebView2未可靠触发document.visibilitychange，性能采样仍续租。补原生Resized最小化事件，同时停止本次camera/pose，前端暂停采样；恢复只重启定位与指标，不自动开启摄像头。用预期ID避免迟到停止误伤新实例；重复恢复清除旧计时器。

工具运行时重载曾中断构建并重置会话；回收保存源码后恢复构建，不复用无效session。functions原始模板内嵌反引号导致两次解析失败，改用落盘脚本/分段处理。一次cargo命令因工具链不在Bash PATH失败，改用已有cargo.py指定Rust/MSVC环境，不重装工具链。Computer Use最小化按钮UIA没有几何时，刷新截图后点击实际标题栏坐标；最小化后状态读取返回minimized为预期结果，不继续用旧坐标。用户按Esc暂停操控后立即停下，得到用户明确继续授权后才恢复。

关键证据：integration-result.json、lifecycle-result.json（修复前另存lifecycle-before-minimize-fix.json）、baseline/optimized8-native/optimized20-native-result.json、core-tests-final.txt、release-final.txt及ui-result.json。发布采用脚本自身目录定位，不复制旧版硬编码目录，上传前核对tag/正文/ZIP文件名/校验和语义一致。

独立审查ship、无material fixes，已按缺失agents目录采用随技能附带备用规范并披露。documenter完成范围内系统记录。安装4份配置字节保持、原生窗口和干净便携检查通过。OpenCV2帧/EOF退出与设备释放通过。UIA文本即时导航滞后，不把旧树当当前画面；刷新原生截图确认。最终八文件ZIP重封装，仅同步说明，校验清单由实际ZIP生成。
