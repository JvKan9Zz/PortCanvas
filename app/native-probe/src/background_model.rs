//! Normalized crop geometry shared by pointer and keyboard operations.
use portcanvas_core::preferences::BackgroundCrop as Crop;
pub fn initial(source_aspect: f64, target_aspect: f64) -> Crop {
    let ratio = target_aspect / source_aspect;
    let width = 1f64.min(ratio);
    let height = width / ratio;
    Crop {
        x: (1. - width) / 2.,
        y: (1. - height) / 2.,
        width,
        height,
    }
}
pub fn moved(c: Crop, dx: f64, dy: f64) -> Crop {
    Crop {
        x: (c.x + dx).clamp(0., 1. - c.width),
        y: (c.y + dy).clamp(0., 1. - c.height),
        ..c
    }
}
pub fn resized(c: Crop, corner: usize, dx: f64, dy: f64, ratio: f64) -> Crop {
    let west = corner == 0 || corner == 2;
    let north = corner < 2;
    let ax = if west { c.x + c.width } else { c.x };
    let ay = if north { c.y + c.height } else { c.y };
    let max = (if west { ax } else { 1. - ax }).min((if north { ay } else { 1. - ay }) * ratio);
    let delta = if dx.abs() >= (dy * ratio).abs() {
        dx * if west { -1. } else { 1. }
    } else {
        dy * ratio * if north { -1. } else { 1. }
    };
    let width = (c.width + delta).clamp(0.03f64.min(max), max);
    let height = width / ratio;
    Crop {
        x: if west { ax - width } else { ax },
        y: if north { ay - height } else { ay },
        width,
        height,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crops_preserve_aspect_and_source_bounds_for_all_orientations() {
        for source in [0.5, 1., 16. / 9., 3.] {
            let c = initial(source, 1.5);
            assert!((c.width / c.height - 1.5 / source).abs() < 1e-9);
            let m = moved(c, 5., -5.);
            assert!(m.x + m.width <= 1. && m.y >= 0.);
            for corner in 0..4 {
                for delta in [-10., 10.] {
                    let r = resized(c, corner, delta, -delta, 1.5 / source);
                    assert!(
                        r.x >= 0.
                            && r.y >= 0.
                            && r.x + r.width <= 1. + 1e-9
                            && r.y + r.height <= 1. + 1e-9
                    );
                    assert!((r.width / r.height - 1.5 / source).abs() < 1e-9);
                }
            }
        }
    }
}
