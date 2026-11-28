#![cfg(all(target_os = "macos", feature = "gui"))]

#[test]
fn macos_system_cjk_font_resolves_all_ui_glyphs_without_tofu() {
    let family = i_slint_renderer_femtovg::register_macos_system_cjk_fonts()
        .expect("macOS CoreText must resolve and register a system CJK font collection");
    eprintln!("Registered macOS system CJK font family: {family}");
    let sample = "设备连接与管理 热点局域网 远程桌面 文件管理与传输 本地运行日志 设备性能 插件 设置 视频采集 128线程下载器 选择目录 打开目录 确认 取消";
    i_slint_renderer_femtovg::verify_macos_cjk_glyphs(&family, sample)
        .expect("all Chinese UI glyphs must resolve to non-zero glyph IDs and load in FemtoVG");
}
