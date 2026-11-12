use crate::workbench_ui::Workbench;
use portcanvas_core::{preferences::GlassQuality, services::Services};
use slint::ComponentHandle;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};
pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    timer: slint::Timer,
    confirmed: Cell<Option<(GlassQuality, u8)>>,
    writes: Cell<u64>,
    failures: Cell<u64>,
}
fn quality(index: i32) -> GlassQuality {
    match index {
        0 => GlassQuality::Performance,
        2 => GlassQuality::Full,
        _ => GlassQuality::Balanced,
    }
}
fn index(quality: GlassQuality) -> i32 {
    match quality {
        GlassQuality::Performance => 0,
        GlassQuality::Balanced => 1,
        GlassQuality::Full => 2,
    }
}
impl Controller {
    pub fn install(ui: &Workbench, services: Arc<Services>) -> Rc<Self> {
        let c = Rc::new(Self {
            ui: ui.as_weak(),
            services,
            timer: slint::Timer::default(),
            confirmed: Cell::new(None),
            writes: Cell::new(0),
            failures: Cell::new(0),
        });
        match c.services.preferences.lock().unwrap().read() {
            Ok(p) => {
                c.confirmed.set(Some((p.glass_quality, p.glass_opacity)));
                ui.set_glass_quality(index(p.glass_quality));
                ui.set_bg_material(f32::from(p.glass_opacity));
                ui.set_glass_loaded(true);
            }
            Err(error) => {
                eprintln!("Glass preferences: {error:#}");
                ui.set_glass_error("设置读取失败，原文件保留。".into());
            }
        }
        ui.on_glass_controls({
            let c = c.clone();
            move |finished| {
                // Commit after the control event settles, so failure rollback remains visible.
                let weak = Rc::downgrade(&c);
                c.timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(if finished { 0 } else { 300 }),
                    move || {
                        if let Some(c) = weak.upgrade() {
                            c.persist();
                        }
                    },
                );
            }
        });
        c
    }
    fn persist(&self) {
        let (Some(ui), Some(previous)) = (self.ui.upgrade(), self.confirmed.get()) else {
            return;
        };
        let requested = (
            quality(ui.get_glass_quality()),
            ui.get_bg_material().round().clamp(0., 100.) as u8,
        );
        if requested == previous {
            return;
        }
        let result = self
            .services
            .preferences
            .lock()
            .unwrap()
            .save_glass(requested.0, requested.1);
        match result {
            Ok(_) => {
                self.confirmed.set(Some(requested));
                self.writes.set(self.writes.get() + 1);
                ui.set_glass_error("".into());
            }
            Err(error) => {
                self.failures.set(self.failures.get() + 1);
                eprintln!("Glass preferences save: {error:#}");
                ui.set_glass_quality(index(previous.0));
                ui.set_bg_material(f32::from(previous.1));
                ui.set_glass_error("玻璃设置保存失败，已恢复上次值。".into());
            }
        }
    }
    pub fn reload(&self) -> anyhow::Result<()> {
        self.timer.stop();
        let p = self.services.preferences.lock().unwrap().read()?;
        self.confirmed.set(Some((p.glass_quality, p.glass_opacity)));
        if let Some(ui) = self.ui.upgrade() {
            ui.set_glass_quality(index(p.glass_quality));
            ui.set_bg_material(f32::from(p.glass_opacity));
            ui.set_glass_loaded(true);
            ui.set_glass_error("".into());
        }
        Ok(())
    }
    pub fn flush(&self) { self.shutdown(); }
    pub fn shutdown(&self) {
        if self.timer.running() {
            self.timer.stop();
            self.persist();
        }
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"confirmed":self.confirmed.get(),"writes":self.writes.get(),"failures":self.failures.get()})
    }
}
