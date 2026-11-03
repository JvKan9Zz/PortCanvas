//! Background-only projection for the workbench's bounded material surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Match Slint's centered `image-fit: cover`, including the saved source crop.
/// All rectangles use logical window coordinates; DPI affects only FBO resolution.
pub fn project(source: (u32, u32), crop: [f32; 4], window: (f32, f32), rect: Rect) -> [f32; 4] {
    let source_width = source.0 as f32 * crop[2];
    let source_height = source.1 as f32 * crop[3];
    let scale = (window.0 / source_width).max(window.1 / source_height);
    let visible = [
        window.0 / scale / source.0 as f32,
        window.1 / scale / source.1 as f32,
    ];
    let origin = [
        crop[0] + (crop[2] - visible[0]) * 0.5,
        crop[1] + (crop[3] - visible[1]) * 0.5,
    ];
    [
        origin[0] + rect.x / window.0 * visible[0],
        origin[1] + rect.y / window.1 * visible[1],
        rect.width / window.0 * visible[0],
        rect.height / window.1 * visible[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: [f32; 4], b: [f32; 4]) {
        for (a, b) in a.into_iter().zip(b) {
            assert!((a - b).abs() < 0.00001, "{a} != {b}");
        }
    }
    #[test]
    fn wide_source_center_cover_and_adjacent_panels_share_coordinates() {
        let left = project(
            (2000, 1000),
            [0., 0., 1., 1.],
            (1000., 1000.),
            Rect {
                x: 0.,
                y: 100.,
                width: 100.,
                height: 800.,
            },
        );
        let next = project(
            (2000, 1000),
            [0., 0., 1., 1.],
            (1000., 1000.),
            Rect {
                x: 100.,
                y: 100.,
                width: 900.,
                height: 800.,
            },
        );
        close(left, [0.25, 0.1, 0.05, 0.8]);
        assert!((left[0] + left[2] - next[0]).abs() < 0.00001);
    }
    #[test]
    fn saved_crop_and_logical_geometry_project_without_dpi_drift() {
        let rect = Rect {
            x: 100.,
            y: 200.,
            width: 400.,
            height: 100.,
        };
        let crop = [0.2, 0.1, 0.5, 0.5];
        let value = project((1600, 900), crop, (800., 450.), rect);
        close(value, [0.2625, 0.32222223, 0.25, 0.11111111]);
        close(
            value,
            project(
                (1600, 900),
                crop,
                (1600., 900.),
                Rect {
                    x: 200.,
                    y: 400.,
                    width: 800.,
                    height: 200.,
                },
            ),
        );
    }
}
