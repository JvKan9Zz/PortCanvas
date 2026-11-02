use anyhow::{ensure, Result};
use image::{GenericImageView, ImageDecoder};
use portcanvas_core::preferences::BackgroundCrop;
use std::path::Path;
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
pub fn load(path: &Path, crop: Option<BackgroundCrop>, limit: u32) -> Result<Picture> {
    let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    ensure!(
        w > 0 && h > 0 && u64::from(w) * u64::from(h) * 4 <= 128 * 1024 * 1024,
        "图片像素超出显示预算。"
    );
    let orientation = decoder.orientation()?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if let Some(c) = crop {
        let (w, h) = image.dimensions();
        let x = (c.x * w as f64).floor() as u32;
        let y = (c.y * h as f64).floor() as u32;
        let width = (c.width * w as f64).round().max(1.) as u32;
        let height = (c.height * h as f64).round().max(1.) as u32;
        ensure!(x < w && y < h, "裁剪范围无效。");
        image = image.crop_imm(x, y, width.min(w - x), height.min(h - y));
    }
    if image.width() > limit || image.height() > limit {
        image = image.resize(limit, limit, image::imageops::FilterType::Triangle);
    }
    let image = image.into_rgba8();
    Ok(Picture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_and_webp_decode_crop_and_release() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
            .join("background-images");
        std::fs::create_dir_all(&root)?;
        for extension in ["png", "jpg", "webp"] {
            let path = root.join(format!("synthetic.{extension}"));
            image::RgbImage::from_fn(80, 40, |x, _| {
                if x < 40 {
                    image::Rgb([255, 0, 0])
                } else {
                    image::Rgb([0, 0, 255])
                }
            })
            .save(&path)?;
            let p = load(
                &path,
                Some(BackgroundCrop {
                    x: 0.5,
                    y: 0.,
                    width: 0.5,
                    height: 1.,
                }),
                2048,
            )?;
            assert_eq!((p.width, p.height), (40, 40));
            assert!(p.rgba[2] > 240);
            std::fs::remove_file(path)?;
        }
        std::fs::remove_dir(root)?;
        Ok(())
    }
}
