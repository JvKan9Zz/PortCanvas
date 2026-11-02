use anyhow::{ensure, Context, Result};
use std::path::PathBuf;
fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?,
    );
    ensure!(!root.exists(), "fresh fixture directory required");
    std::fs::create_dir_all(root.join("data"))?;
    let pixels = image::RgbImage::from_fn(1600, 900, |x, y| {
        image::Rgb([
            if x < 533 { 230 } else { 30 },
            if (533..1066).contains(&x) { 210 } else { 40 },
            if x >= 1066 {
                230
            } else {
                (y * 150 / 900) as u8
            },
        ])
    });
    for extension in ["png", "jpg", "webp"] {
        pixels.save(root.join(format!("crop-test.{extension}")))?;
    }
    portcanvas_native_probe::media::write_av_fixture(&root.join("audio-video.mp4"))?;
    std::fs::write(root.join("broken.png"), b"\x89PNG\r\n\x1a\nnot-an-image")?;
    println!(
        "Isolated image/video fixtures created at {}",
        root.display()
    );
    Ok(())
}
