use crate::{
    material_gpu::{Gpu, Surface},
    workbench_ui::Workbench,
};
use portcanvas_native_probe::glass_model::{project, Rect};
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(PartialEq)]
struct Key {
    revision: u64,
    window: (f32, f32),
    crop: [f32; 4],
    rects: Vec<Rect>,
    quality: i32,
    opacity: f32,
    tint: [u8; 3],
}
pub struct Controller {
    ui: slint::Weak<Workbench>,
    pixels: RefCell<Option<SharedPixelBuffer<Rgba8Pixel>>>,
    revision: Cell<u64>,
    uploaded: Cell<u64>,
    gpu: RefCell<Option<Gpu>>,
    key: RefCell<Option<Key>>,
    bindings: RefCell<Option<((u32, u32), Vec<(u32, u32)>)>>,
    failed: Cell<bool>,
    uploads: Cell<u64>,
    passes: Cell<u64>,
    quality_passes: [Cell<u64>; 3],
    visibility: RefCell<Vec<serde_json::Value>>,
    released: Cell<bool>,
    driver: RefCell<String>,
    error: RefCell<String>,
}
impl Controller {
    pub fn install(ui: &Workbench, pending_draw: Arc<AtomicU64>) -> anyhow::Result<Rc<Self>> {
        let c = Rc::new(Self {
            ui: ui.as_weak(),
            pixels: RefCell::new(None),
            revision: Cell::new(0),
            uploaded: Cell::new(0),
            gpu: RefCell::new(None),
            key: RefCell::new(None),
            bindings: RefCell::new(None),
            failed: Cell::new(false),
            uploads: Cell::new(0),
            passes: Cell::new(0),
            quality_passes: Default::default(),
            visibility: RefCell::new(Vec::new()),
            released: Cell::new(true),
            driver: RefCell::new(String::new()),
            error: RefCell::new(String::new()),
        });
        let render = c.clone();
        let weak = ui.as_weak();
        // One owner for material setup/passes/teardown AND existing desktop frame acknowledgement.
        ui.window()
            .set_rendering_notifier(move |state, api| match state {
                slint::RenderingState::RenderingSetup => {
                    render.failed.set(false);
                    render.uploaded.set(0);
                    render.key.borrow_mut().take();
                    render.bindings.borrow_mut().take();
                }
                slint::RenderingState::BeforeRendering => render.draw(api),
                slint::RenderingState::AfterRendering => {
                    let ticket = pending_draw.swap(0, Ordering::SeqCst);
                    if ticket != 0 {
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.invoke_desktop_drawn(ticket.to_string().into())
                        });
                    }
                }
                slint::RenderingState::RenderingTeardown => render.release(),
                _ => {}
            })
            .map_err(|e| anyhow::anyhow!("原生绘制通知不可用：{e:?}"))?;
        Ok(c)
    }
    /// Share the same CPU buffer used by the ordinary Slint fallback; never decode twice.
    pub fn publish(&self, pixels: SharedPixelBuffer<Rgba8Pixel>) {
        if let Some(ui) = self.ui.upgrade() {
            if !ui.get_material_active() {
                ui.set_bg_frame(slint::Image::from_rgba8(pixels.clone()));
            }
            *self.pixels.borrow_mut() = Some(pixels);
            self.revision.set(self.revision.get() + 1);
            ui.window().request_redraw();
        }
    }
    pub fn clear(&self) {
        self.pixels.borrow_mut().take();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_material_active(false);
            ui.set_bg_frame(Default::default());
            ui.set_material_nav(Default::default());
            ui.set_material_header(Default::default());
            ui.set_material_terminal(Default::default());
            ui.window().request_redraw();
        }
    }
    fn release(&self) {
        if let Some(gpu) = self.gpu.borrow_mut().take() {
            unsafe {
                gpu.destroy();
            }
        }
        self.uploaded.set(0);
        self.bindings.borrow_mut().take();
        self.key.borrow_mut().take();
        self.released.set(true);
    }
    fn draw(&self, api: &slint::GraphicsAPI) {
        let Some(ui) = self.ui.upgrade() else { return };
        if ui.get_native_hidden() {
            return;
        }
        let pixels = self.pixels.borrow();
        let Some(pixels) = pixels.as_ref() else {
            self.release();
            return;
        };
        if self.failed.get() {
            return;
        }
        let rect = |r: crate::workbench_ui::MaterialRect| Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        };
        let mut rects = vec![
            rect(ui.get_material_nav_rect()),
            rect(ui.get_material_header_rect()),
        ];
        if ui.get_terminal_visible() {
            rects.push(rect(ui.get_material_terminal_rect()));
        }
        if rects.iter().any(|r| r.width <= 0. || r.height <= 0.) {
            return;
        }
        let tint = ui.get_material_tint();
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let key = Key {
            revision: self.revision.get(),
            window: (size.width, size.height),
            rects,
            crop: [
                ui.get_bg_saved_crop_x(),
                ui.get_bg_saved_crop_y(),
                ui.get_bg_saved_crop_width(),
                ui.get_bg_saved_crop_height(),
            ],
            quality: ui.get_glass_quality(),
            opacity: ui.get_bg_material(),
            tint: [tint.red(), tint.green(), tint.blue()],
        };
        if self.key.borrow().as_ref() == Some(&key) {
            return;
        }
        let source = (pixels.width(), pixels.height());
        let surfaces = key
            .rects
            .iter()
            .map(|r| Surface {
                // Material carries no text; logical-size targets bound fill cost at high DPI.
                size: (r.width.ceil() as u32, r.height.ceil() as u32),
                crop: project(source, key.crop, key.window, *r),
            })
            .collect::<Vec<_>>();
        let result = (|| unsafe {
            let mut slot = self.gpu.borrow_mut();
            if slot.is_none() {
                let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
                    anyhow::bail!("Native OpenGL unavailable")
                };
                let gl = glow::Context::from_loader_function(|name| {
                    get_proc_address(&std::ffi::CString::new(name).unwrap())
                });
                let gpu = Gpu::new(gl)?;
                *self.driver.borrow_mut() = gpu.driver.clone();
                self.released.set(false);
                *slot = Some(gpu);
            }
            let gpu = slot.as_mut().unwrap();
            let frame = (self.uploaded.get() != key.revision)
                .then(|| (source.0, source.1, pixels.as_bytes()));
            let images = gpu.draw(
                frame,
                &surfaces,
                key.quality,
                key.opacity,
                key.tint.map(|v| v as f32 / 255.),
            )?;
            self.uploads
                .set(self.uploads.get() + u64::from(frame.is_some()));
            self.passes.set(self.passes.get() + surfaces.len() as u64);
            self.quality_passes[key.quality.clamp(0, 2) as usize].set(
                self.quality_passes[key.quality.clamp(0, 2) as usize].get() + surfaces.len() as u64,
            );
            self.uploaded.set(key.revision);
            Ok::<_, anyhow::Error>(images)
        })();
        match result {
            Ok((background, images)) => {
                let binding = (source, surfaces.iter().map(|s| s.size).collect::<Vec<_>>());
                if self.bindings.borrow().as_ref() != Some(&binding) {
                    ui.set_bg_frame(background);
                    ui.set_material_nav(images[0].clone());
                    ui.set_material_header(images[1].clone());
                    ui.set_material_terminal(images.get(2).cloned().unwrap_or_default());
                    *self.bindings.borrow_mut() = Some(binding);
                }
                ui.set_material_active(true);
                *self.key.borrow_mut() = Some(key);
            }
            Err(error) => {
                eprintln!("Workbench material fallback: {error:#}");
                *self.error.borrow_mut() = error.to_string();
                self.failed.set(true);
                ui.set_material_active(false);
                ui.set_bg_frame(slint::Image::from_rgba8(pixels.clone()));
                ui.set_material_nav(Default::default());
                ui.set_material_header(Default::default());
                ui.set_material_terminal(Default::default());
                self.release();
            }
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"sourceUploads":self.uploads.get(),"materialPasses":self.passes.get(),
            "qualityPasses":self.quality_passes.each_ref().map(Cell::get),"visibility":*self.visibility.borrow(),
            "released":self.released.get(),"driver":*self.driver.borrow(),"error":*self.error.borrow()})
    }
    pub fn visibility(&self, hidden: bool) {
        if std::env::var_os("PORTCANVAS_WORKBENCH_REPORT").is_some() {
            self.visibility.borrow_mut().push(serde_json::json!({"hidden":hidden,"sourceUploads":self.uploads.get(),"materialPasses":self.passes.get()}));
        }
    }
}
