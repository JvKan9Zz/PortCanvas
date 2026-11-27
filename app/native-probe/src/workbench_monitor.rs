use crate::{
    workbench::text,
    workbench_ui::{CoreRow, FactRow, MonitorKpi, NetRow, StorageRow, Workbench},
};
use portcanvas_core::services::Services;
use portcanvas_native_probe::{
    file_browser::bytes,
    monitor_model::{Metrics, Reading, Usage},
};
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{runtime::Runtime, sync::Notify, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const MISSING: &str = "—";

struct Watch {
    generation: u64,
    ticket: u64,
    cancel: CancellationToken,
    refresh: Arc<Notify>,
}

pub struct Controller {
    ui: slint::Weak<Workbench>,
    services: Arc<Services>,
    runtime: Arc<Runtime>,
    jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    watch: RefCell<Option<Watch>>,
    ticket: Cell<u64>,
    metrics: RefCell<Metrics>,
    latest: RefCell<Option<Reading>>,
    aspect: Cell<f64>,
    phase: Cell<i32>,
    reads: Arc<AtomicU64>,
}

impl Controller {
    pub fn install(
        ui: &Workbench,
        services: Arc<Services>,
        runtime: Arc<Runtime>,
        jobs: Arc<Mutex<Vec<JoinHandle<()>>>>,
    ) -> Rc<Self> {
        let controller = Rc::new(Self {
            ui: ui.as_weak(),
            services,
            runtime,
            jobs,
            watch: RefCell::new(None),
            ticket: Cell::new(0),
            metrics: RefCell::new(Metrics::default()),
            latest: RefCell::new(None),
            aspect: Cell::new(10.),
            phase: Cell::new(-1),
            reads: Arc::new(AtomicU64::new(0)),
        });
        ui.on_monitor_sync({
            let controller = controller.clone();
            move || controller.sync()
        });
        ui.on_monitor_refresh({
            let controller = controller.clone();
            move || {
                if let Some(watch) = controller.watch.borrow().as_ref() {
                    watch.refresh.notify_one();
                } else {
                    controller.sync();
                }
            }
        });
        ui.on_monitor_plot_changed({
            let controller = controller.clone();
            move |aspect| {
                if aspect.is_finite() && aspect > 0. {
                    controller.aspect.set(f64::from(aspect));
                    controller.render_paths();
                }
            }
        });
        ui.on_monitor_metrics_result({
            let controller = controller.clone();
            move |ticket, payload| controller.receive(&ticket, &payload)
        });
        controller.sync();
        controller
    }

    pub fn sync(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let generation = self.services.devices.generation();
        let active = ui.get_page() == 8
            && !ui.get_native_hidden()
            && ui.get_connected()
            && !self.services.quitting.load(Ordering::SeqCst);
        if !active && self.phase.get() == 0
            && self.watch.borrow().is_none() && self.latest.borrow().is_none()
        {
            return;
        }
        if active
            && self
                .watch
                .borrow()
                .as_ref()
                .is_some_and(|watch| watch.generation == generation)
        {
            self.render(&ui);
            return;
        }
        self.shutdown();
        self.metrics.borrow_mut().clear();
        self.latest.borrow_mut().take();
        self.phase.set(if active { 1 } else { 0 });
        self.render(&ui);
        if !active {
            return;
        }

        let ticket = self.ticket.get() + 1;
        self.ticket.set(ticket);
        let cancel = CancellationToken::new();
        let refresh = Arc::new(Notify::new());
        *self.watch.borrow_mut() = Some(Watch {
            generation,
            ticket,
            cancel: cancel.clone(),
            refresh: refresh.clone(),
        });
        let services = self.services.clone();
        let weak = self.ui.clone();
        let reads = self.reads.clone();
        let task = self.runtime.spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => break,
                    _ = refresh.notified() => tick.reset(),
                    _ = tick.tick() => {}
                }
                reads.fetch_add(1, Ordering::Relaxed);
                let value = services
                    .arena
                    .metrics_for_generation(generation, &cancel)
                    .await
                    .unwrap_or_else(|error| json!({"error":error.to_string()}));
                if cancel.is_cancelled()
                    || weak
                        .upgrade_in_event_loop(move |ui| {
                            ui.invoke_monitor_metrics_result(
                                ticket.to_string().into(),
                                value.to_string().into(),
                            );
                        })
                        .is_err()
                {
                    break;
                }
            }
        });
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|job| !job.is_finished());
        jobs.push(task);
    }

    fn receive(&self, ticket: &str, payload: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        if self.watch.borrow().as_ref().is_none_or(|watch| {
            ticket.parse().ok() != Some(watch.ticket)
                || watch.generation != self.services.devices.generation()
        }) {
            return;
        }
        let reading = serde_json::from_str(payload)
            .ok()
            .and_then(|value| self.metrics.borrow_mut().push(value));
        self.phase.set(if reading.is_some() { 2 } else { 3 });
        if reading.is_none() {
            self.metrics.borrow_mut().failed();
        }
        *self.latest.borrow_mut() = reading;
        self.render(&ui);
    }

    fn render_paths(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let metrics = self.metrics.borrow();
        ui.set_monitor_cpu_path(metrics.path_for_aspect(false, self.aspect.get()).into());
        ui.set_monitor_memory_path(metrics.memory_dashes_for_aspect(self.aspect.get()).into());
    }

    fn render(&self, ui: &Workbench) {
        let english = ui.get_english();
        let tr = |label| text(label, english);
        let phase = self.phase.get();
        ui.set_monitor_status_phase(phase);
        ui.set_monitor_status(
            match phase {
                1 => "正在读取…",
                2 => "实时",
                3 => "读取失败",
                _ => "设备未连接",
            }
            .into(),
        );

        let latest = self.latest.borrow();
        let reading = latest.as_ref();
        let root =
            reading.and_then(|reading| reading.storage.iter().find(|disk| disk.mount == "/"));
        let thermal = reading.and_then(|reading| {
            reading
                .thermals
                .iter()
                .find(|zone| zone.label.to_ascii_lowercase().contains("cpu"))
                .or_else(|| reading.thermals.first())
        });
        let rtt = reading
            .and_then(|reading| reading.round_trip_ms)
            .map(|ms| format!("{ms:.0} ms"));
        let cores = reading
            .and_then(|reading| reading.core_count)
            .map(|count| format!("{count} {}", tr("核")));
        let frequency = reading
            .and_then(|reading| reading.cpu_freq_mhz)
            .map(|mhz| format!("{:.2} GHz", mhz / 1000.));
        let kpis = vec![
            kpi(
                tr("CPU"),
                reading.and_then(|r| r.cpu),
                join(cores.iter().chain(frequency.iter()).map(String::as_str)),
                0,
            ),
            kpi(
                tr("内存"),
                reading.map(|r| r.memory),
                reading.map(|r| usage(r.memory_bytes)).unwrap_or_default(),
                1,
            ),
            kpi(
                tr("存储"),
                root.map(|disk| disk.usage.percent()),
                root.map(|disk| usage(disk.usage)).unwrap_or_default(),
                root.map_or(0, |disk| disk.tone()),
            ),
            MonitorKpi {
                label: tr(if thermal.is_some() {
                    "温度"
                } else {
                    "往返时间"
                })
                .into(),
                value: thermal
                    .map(|zone| format!("{:.1} °C", zone.celsius))
                    .or_else(|| rtt.clone())
                    .unwrap_or_else(|| MISSING.into())
                    .into(),
                detail: join(
                    thermal
                        .and(rtt.as_ref())
                        .map(|value| format!("SSH {value}"))
                        .into_iter()
                        .chain(
                            reading
                                .and_then(|r| r.load)
                                .map(|load| format!("{} {:.2}", tr("负载"), load[0])),
                        )
                        .collect::<Vec<_>>()
                        .iter()
                        .map(String::as_str),
                )
                .into(),
                usage: -1.,
                tone: 0,
            },
        ];
        ui.set_monitor_kpis(model(kpis));
        ui.set_monitor_cores(model(
            reading
                .into_iter()
                .flat_map(|r| &r.cores)
                .map(|core| CoreRow {
                    label: format!("C{}", core.id).into(),
                    usage: core.usage.map_or(-1., |value| value as f32),
                })
                .collect(),
        ));
        ui.set_monitor_storage(model(
            reading
                .into_iter()
                .flat_map(|r| &r.storage)
                .map(|disk| StorageRow {
                    mount: disk.mount.as_str().into(),
                    fs: disk.fs_type.as_str().into(),
                    summary: usage(disk.usage).into(),
                    free: format!("{} {}", tr("可用"), bytes(disk.available)).into(),
                    percent: format!("{:.0}%", disk.usage.percent()).into(),
                    usage: disk.usage.percent() as f32,
                    tone: disk.tone(),
                })
                .collect(),
        ));
        ui.set_monitor_networks(model(
            reading
                .into_iter()
                .flat_map(|r| &r.networks)
                .map(|nic| NetRow {
                    name: nic.name.as_str().into(),
                    rx: rate(nic.rx_rate).into(),
                    tx: rate(nic.tx_rate).into(),
                    total: format!(
                        "↓ {} · ↑ {}",
                        bytes(nic.received as u64),
                        bytes(nic.sent as u64)
                    )
                    .into(),
                })
                .collect(),
        ));
        ui.set_monitor_rx_rate(rate(reading.and_then(|r| r.received)).into());
        ui.set_monitor_tx_rate(rate(reading.and_then(|r| r.sent)).into());
        ui.set_monitor_rtt(rtt.unwrap_or_else(|| MISSING.into()).into());

        let system = reading.and_then(|r| r.system.as_ref());
        let os = system.and_then(|system| system.os.as_deref());
        let arch = system.and_then(|system| system.arch.as_deref());
        let host = system.and_then(|system| system.hostname.as_deref());
        let kernel = system.and_then(|system| system.kernel.as_deref());
        ui.set_monitor_host_summary(
            join(os.into_iter().chain(arch).chain(cores.as_deref())).into(),
        );
        let facts = [
            ("主机名", host.unwrap_or(MISSING).to_owned()),
            ("系统", os.unwrap_or(MISSING).to_owned()),
            ("内核与架构", join(kernel.into_iter().chain(arch))),
            (
                "运行时间",
                reading
                    .and_then(|r| r.uptime)
                    .map(uptime)
                    .unwrap_or_default(),
            ),
            (
                "平均负载",
                reading
                    .and_then(|r| r.load)
                    .map(|v| format!("{:.2} / {:.2} / {:.2}", v[0], v[1], v[2]))
                    .unwrap_or_default(),
            ),
            (
                "交换空间",
                reading.and_then(|r| r.swap).map(usage).unwrap_or_default(),
            ),
        ];
        let mut facts: Vec<_> = facts
            .into_iter()
            .map(|(label, value)| FactRow {
                label: tr(label).into(),
                value: if value.is_empty() {
                    MISSING.into()
                } else {
                    value.into()
                },
            })
            .collect();
        if let Some(reading) = reading.filter(|reading| !reading.thermals.is_empty()) {
            facts.push(FactRow {
                label: tr("传感器温度").into(),
                value: reading
                    .thermals
                    .iter()
                    .map(|zone| format!("{} {:.1} °C", zone.label, zone.celsius))
                    .collect::<Vec<_>>()
                    .join(" · ")
                    .into(),
            });
        }
        ui.set_monitor_facts(model(facts));
        self.render_paths();
    }

    pub fn shutdown(&self) {
        if let Some(watch) = self.watch.borrow_mut().take() {
            watch.cancel.cancel();
        }
    }

    pub fn report(&self) -> Value {
        json!({"metricsRequests":self.reads.load(Ordering::Relaxed),"active":self.watch.borrow().is_some()})
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn kpi(label: String, percent: Option<f64>, detail: String, tone: i32) -> MonitorKpi {
    MonitorKpi {
        label: label.into(),
        detail: detail.into(),
        tone,
        value: percent
            .map(|v| format!("{v:.1}%"))
            .unwrap_or_else(|| MISSING.into())
            .into(),
        usage: percent.map_or(-1., |v| v as f32),
    }
}

fn join<'a>(words: impl Iterator<Item = &'a str>) -> String {
    words
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn usage(usage: Usage) -> String {
    format!("{} / {}", bytes(usage.used), bytes(usage.total))
}

fn rate(value: Option<f64>) -> String {
    value
        .map(|value| format!("{}/s", bytes(value as u64)))
        .unwrap_or_else(|| MISSING.into())
}

fn uptime(seconds: f64) -> String {
    let minutes = (seconds / 60.) as u64;
    if minutes >= 1440 {
        format!("{} d {} h", minutes / 1440, (minutes / 60) % 24)
    } else if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}
