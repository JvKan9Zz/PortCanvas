# PortCanvas 品牌资源

原始标志与图标由用户于 2026-10-23 提供，原件保持。portcanvas-icon.png 由内置 imagegen 从原图下方应用图标提取为透明背景方形资源，用于后续 Windows ICO 和应用标识；没有重新设计 Logo。

提取提示：仅保留下方深色圆角方块、蓝色细描边和原银白/冰蓝 PC 接口图形，裁去上方文字与外围画板背景，保持形状、孔位、比例与配色。

用户随后要求视觉比例改为正方形，并用Logo替换侧栏W。portcanvas-icon-square.png由内置imagegen基于提取图修正外框比例、清理透明边缘，保留银白P/冰蓝C和接口圆环；两份原始资源均保留。正式PNG/ICO由官方Tauri icon转换，侧栏128×128资源按48×48显示，窗口/任务栏使用多尺寸ICO。

V0.4曾使用portcanvas-icon-final.png：内置imagegen保留PC接口图形，移除偏宽的内框，用深色底板铺满1:1画布。该版本现仅作历史来源保留。

Demo V0.4.5以用户最终指定的透明底图portcanvas-transparent.png为统一来源，原样复制自本机generated_images中的exec-e9a5ddf9-2d5b-4005-af5b-404e531ec692.png。源文件1254×1254、RGBA，四角透明；没有重新绘制或改变PC图形。官方Tauri icon工具等比生成32/128/256 PNG及多尺寸ICO，应用侧栏、窗口、任务栏、安装包、桌面快捷方式和README统一使用此版本。外画布保持1:1，图形自身比例按用户选图保留。安装快捷方式引用带版本号的独立ICO，避免旧EXE路径图标缓存；不清理全局Windows缓存。
