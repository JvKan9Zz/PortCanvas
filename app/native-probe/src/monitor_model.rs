//! Device performance data and 60-second CPU/memory history.
use serde::Deserialize;
use serde_json::Value;
use std::collections::VecDeque;

#[derive(Default)]
pub struct Metrics {
    previous: Option<Sample>,
    generation: Option<u64>,
    points: VecDeque<(f64, Option<f64>, Option<f64>)>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    clock: f64,
    generation: Option<u64>,
    cpu_total: Option<u64>,
    cpu_idle: Option<u64>,
    cores: Option<u32>,
    #[serde(default, deserialize_with = "nullable_array")]
    cpu_cores: Vec<CpuTicks>,
    cpu_freq_mhz: Option<f64>,
    memory_total: u64,
    memory_available: u64,
    memory_cached: Option<u64>,
    swap_total: Option<u64>,
    swap_free: Option<u64>,
    #[serde(default, deserialize_with = "nullable_array")]
    network: Vec<NetCounters>,
    #[serde(default, deserialize_with = "nullable_array")]
    storage: Vec<StorageCounters>,
    #[serde(default, deserialize_with = "nullable_array")]
    thermals: Vec<Thermal>,
    system: Option<SystemFacts>,
    uptime: Option<f64>,
    load: Option<[f64; 3]>,
    round_trip_ms: Option<f64>,
}

fn nullable_array<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Vec<T>, D::Error> {
    Option::<Vec<T>>::deserialize(input).map(Option::unwrap_or_default)
}

#[derive(Deserialize)]
struct CpuTicks {
    id: u32,
    total: u64,
    idle: u64,
}

#[derive(Deserialize)]
struct NetCounters {
    name: String,
    received: f64,
    sent: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageCounters {
    mount: String,
    device: String,
    fs_type: String,
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
}

#[derive(Clone, Deserialize)]
pub struct Thermal {
    pub label: String,
    pub celsius: f64,
}

#[derive(Clone, Deserialize)]
pub struct SystemFacts {
    pub hostname: Option<String>,
    pub os: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
}

#[derive(Clone, Copy)]
pub struct Usage {
    pub used: u64,
    pub total: u64,
}

impl Usage {
    pub fn percent(self) -> f64 {
        100. * self.used as f64 / self.total as f64
    }

    fn from_available(total: u64, available: u64) -> Option<Self> {
        (total > 0).then_some(Self {
            total,
            used: total.checked_sub(available)?,
        })
    }
}

pub struct CoreReading {
    pub id: u32,
    pub usage: Option<f64>,
}

pub struct StorageReading {
    pub mount: String,
    pub device: String,
    pub fs_type: String,
    pub usage: Usage,
    pub available: u64,
}

impl StorageReading {
    pub fn tone(&self) -> i32 {
        match self.usage.percent() {
            value if value >= 90. => 3,
            value if value >= 75. => 2,
            _ => 0,
        }
    }
}

pub struct NetworkReading {
    pub name: String,
    pub received: f64,
    pub sent: f64,
    pub rx_rate: Option<f64>,
    pub tx_rate: Option<f64>,
}

pub struct Reading {
    pub cpu: Option<f64>,
    pub memory: f64,
    pub received: Option<f64>,
    pub sent: Option<f64>,
    pub cores: Vec<CoreReading>,
    pub core_count: Option<u32>,
    pub cpu_freq_mhz: Option<f64>,
    pub memory_bytes: Usage,
    pub memory_cached: Option<u64>,
    pub swap: Option<Usage>,
    pub storage: Vec<StorageReading>,
    pub networks: Vec<NetworkReading>,
    pub thermals: Vec<Thermal>,
    pub system: Option<SystemFacts>,
    pub uptime: Option<f64>,
    pub load: Option<[f64; 3]>,
    pub round_trip_ms: Option<f64>,
}

fn cpu_usage(total: u64, idle: u64, old_total: u64, old_idle: u64) -> Option<f64> {
    Usage::from_available(total.checked_sub(old_total)?, idle.checked_sub(old_idle)?)
        .map(Usage::percent)
}

fn rate(value: f64, previous: f64, seconds: f64) -> Option<f64> {
    (value >= previous && previous >= 0.).then_some((value - previous) / seconds)
}

impl Metrics {
    pub fn push(&mut self, value: Value) -> Option<Reading> {
        let sample: Sample = serde_json::from_value(value).ok()?;
        let memory = Usage::from_available(sample.memory_total, sample.memory_available)?;
        let old = self.previous.as_ref().and_then(|old| {
            let elapsed = sample.clock - old.clock;
            (sample.generation == old.generation && elapsed > 0. && elapsed < 15.)
                .then_some((old, elapsed))
        });
        let restarted = self.generation != sample.generation
            || self
                .points
                .back()
                .is_some_and(|point| sample.clock <= point.0);
        if restarted || (self.previous.is_some() && old.is_none()) {
            self.points.clear();
        }
        let cpu = old.and_then(|(old, _)| {
            cpu_usage(
                sample.cpu_total?,
                sample.cpu_idle?,
                old.cpu_total?,
                old.cpu_idle?,
            )
        });
        let cores = sample
            .cpu_cores
            .iter()
            .map(|core| CoreReading {
                id: core.id,
                usage: old.and_then(|(old, _)| {
                    let previous = old.cpu_cores.iter().find(|item| item.id == core.id)?;
                    cpu_usage(core.total, core.idle, previous.total, previous.idle)
                }),
            })
            .collect();
        let networks: Vec<_> = sample
            .network
            .iter()
            .filter(|nic| nic.received >= 0. && nic.sent >= 0.)
            .map(|nic| {
                let previous = old.and_then(|(old, dt)| {
                    old.network
                        .iter()
                        .find(|item| item.name == nic.name)
                        .map(|nic| (nic, dt))
                });
                NetworkReading {
                    name: nic.name.clone(),
                    received: nic.received,
                    sent: nic.sent,
                    rx_rate: previous.and_then(|(old, dt)| rate(nic.received, old.received, dt)),
                    tx_rate: previous.and_then(|(old, dt)| rate(nic.sent, old.sent, dt)),
                }
            })
            .collect();
        let total = |select: fn(&NetworkReading) -> Option<f64>| {
            networks
                .iter()
                .filter_map(select)
                .reduce(|sum, value| sum + value)
        };
        let storage = sample
            .storage
            .iter()
            .filter(|disk| {
                disk.total_bytes > 0
                    && disk.used_bytes <= disk.total_bytes
                    && disk.available_bytes <= disk.total_bytes
            })
            .map(|disk| StorageReading {
                mount: disk.mount.clone(),
                device: disk.device.clone(),
                fs_type: disk.fs_type.clone(),
                usage: Usage {
                    used: disk.used_bytes,
                    total: disk.total_bytes,
                },
                available: disk.available_bytes,
            })
            .take(6)
            .collect();
        let reading = Reading {
            cpu,
            memory: memory.percent(),
            memory_bytes: memory,
            received: total(|nic| nic.rx_rate),
            sent: total(|nic| nic.tx_rate),
            cores,
            core_count: sample.cores,
            cpu_freq_mhz: sample.cpu_freq_mhz.filter(|hz| *hz > 0.),
            memory_cached: sample
                .memory_cached
                .filter(|cached| *cached <= memory.total),
            swap: sample
                .swap_total
                .zip(sample.swap_free)
                .and_then(|(total, free)| Usage::from_available(total, free)),
            storage,
            networks,
            thermals: sample
                .thermals
                .iter()
                .filter(|zone| (-40. ..=150.).contains(&zone.celsius))
                .cloned()
                .collect(),
            system: sample.system.clone(),
            uptime: sample.uptime.filter(|value| *value >= 0.),
            load: sample.load,
            round_trip_ms: sample.round_trip_ms.filter(|value| *value >= 0.),
        };
        self.points
            .push_back((sample.clock, cpu, Some(reading.memory)));
        while self
            .points
            .front()
            .is_some_and(|point| sample.clock - point.0 > 60.)
        {
            self.points.pop_front();
        }
        self.generation = sample.generation;
        self.previous = Some(sample);
        Some(reading)
    }
}

impl Metrics {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn failed(&mut self) {
        self.previous = None;
        if let Some(p) = self
            .points
            .back()
            .filter(|p| p.1.is_some() || p.2.is_some())
        {
            self.points.push_back((p.0 + 0.001, None, None));
        }
    }
    pub fn path(&self, memory: bool) -> String {
        self.path_for_aspect(memory, 10.)
    }
    /// Dashed memory trace, with the same missing-sample and time-gap rules.
    pub fn memory_dashes_for_aspect(&self, aspect: f64) -> String {
        let Some(end) = self.points.back().map(|p| p.0) else {
            return String::new();
        };
        let mut path = String::new();
        let mut previous = None::<(f64, f64, f64)>;
        for &(t, _, memory) in &self.points {
            let Some(value) = memory else {
                previous = None;
                continue;
            };
            let x = (100. - (end - t) / 60. * 100.).clamp(0., 100.) * aspect / 10.;
            let y = (100. - value.clamp(0., 100.)) / 10.;
            if let Some((time, a, b)) = previous.filter(|(time, _, _)| t - time < 8.) {
                let _ = time;
                let dx = x - a;
                let dy = y - b;
                let length = dx.hypot(dy);
                let mut offset = 0.;
                while offset < length {
                    let start = offset / length;
                    let finish = (offset + 0.5).min(length) / length;
                    path.push_str(&format!(
                        "M{:.2},{:.2} L{:.2},{:.2} ",
                        a + dx * start,
                        b + dy * start,
                        a + dx * finish,
                        b + dy * finish
                    ));
                    offset += 0.875;
                }
            } else {
                path.push_str(&format!("M{x:.2},{y:.2} "));
            }
            previous = Some((t, x, y));
        }
        path
    }
    pub fn path_for_aspect(&self, memory: bool, aspect: f64) -> String {
        let Some(end) = self.points.back().map(|p| p.0) else {
            return String::new();
        };
        let mut path = String::new();
        let mut last = None;
        for &(t, cpu, mem) in &self.points {
            if let Some(value) = if memory { mem } else { cpu } {
                let command = if last.is_some_and(|p| t - p < 8.) {
                    'L'
                } else {
                    'M'
                };
                path.push_str(&format!(
                    "{command}{:.2},{:.2} ",
                    (100. - (end - t) / 60. * 100.).clamp(0., 100.) * aspect / 10.,
                    (100. - value.clamp(0., 100.)) / 10.
                ));
                last = Some(t);
            } else {
                last = None;
            }
        }
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashed_memory_does_not_bridge_missing_or_stalled_samples() {
        let mut metrics = Metrics::default();
        metrics.points.extend([
            (1., None, Some(25.)),
            (3., None, Some(25.)),
            (4., None, None),
            (5., None, Some(50.)),
            (20., None, Some(75.)),
        ]);
        let path = metrics.memory_dashes_for_aspect(3.75);
        assert!(path.contains(" L"));
        assert!(path.ends_with("M37.50,2.50 "));
        // The isolated value after the missing sample remains a move, not a line.
        assert!(path.contains("M28.12,5.00 "));
    }
    #[test]
    fn history_coordinates_follow_the_viewport_aspect_and_keep_percentage_units() {
        let mut metrics = Metrics::default();
        metrics.points.push_back((10., Some(50.), Some(25.)));
        for (aspect, right) in [(1., 10.), (3.75, 37.5), (10., 100.)] {
            assert_eq!(
                metrics.path_for_aspect(false, aspect),
                format!("M{right:.2},5.00 ")
            );
            assert_eq!(
                metrics.path_for_aspect(true, aspect),
                format!("M{right:.2},7.50 ")
            );
        }
    }
    use serde_json::json;
    fn sample(clock: f64, cpu: u64, idle: u64) -> Value {
        json!({"clock":clock,"generation":1,"cpuTotal":cpu,"cpuIdle":idle,"memoryTotal":1000,"memoryAvailable":250,"network":[{"name":"eth0","received":clock*100.,"sent":clock*50.}]})
    }
    #[test]
    fn rates_need_same_generation_monotonic_counters_and_a_bounded_interval() {
        let mut m = Metrics::default();
        let r = m.push(sample(1., 100, 50)).unwrap();
        assert!(r.cpu.is_none());
        let r = m.push(sample(3., 200, 75)).unwrap();
        assert_eq!(r.cpu, Some(75.));
        assert_eq!(r.received, Some(100.));
        let mut v = sample(5., 300, 100);
        v["generation"] = json!(2);
        assert!(m.push(v).unwrap().cpu.is_none());
        assert!(m.push(sample(30., 400, 120)).unwrap().cpu.is_none());
    }
    #[test]
    fn missing_samples_break_lines_and_history_is_bounded() {
        let mut m = Metrics::default();
        m.push(sample(1., 100, 50));
        m.push(sample(3., 200, 75));
        m.failed();
        for _ in 0..100 {
            m.failed();
        }
        assert_eq!(m.points.len(), 3, "repeated failures keep one gap marker");
        m.push(sample(5., 300, 100));
        assert_eq!(m.path(true).matches('M').count(), 2);
        m.push(sample(70., 400, 150));
        assert!(m.points.iter().all(|p| p.0 >= 10.));
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expanded_samples_compute_by_core_and_interface_and_preserve_system_readings() {
        let mut first = json!({
            "clock": 1., "generation": 10,
            "cpuTotal": 1000, "cpuIdle": 600, "cores": 2,
            "cpuCores": [{"id":0,"total":500,"idle":300},{"id":1,"total":500,"idle":300}],
            "cpuFreqMhz": 1800.,
            "memoryTotal": 4096, "memoryAvailable": 1024, "memoryCached": 512,
            "swapTotal": 2048, "swapFree": 512, "uptime": 123.,
            "load": [0.1, 0.2, 0.3], "roundTripMs": 14.,
            "network": [{"name":"eth0","received":500,"sent":100},{"name":"wlan0","received":400,"sent":300}],
            "storage": [
                {"mount":"/","device":"/dev/root","fsType":"ext4","totalBytes":1000,"usedBytes":749,"availableBytes":200},
                {"mount":"/home","device":"/dev/sda1","fsType":"ext4","totalBytes":1000,"usedBytes":750,"availableBytes":150},
                {"mount":"/boot","device":"/dev/sda2","fsType":"vfat","totalBytes":1000,"usedBytes":900,"availableBytes":100}
            ],
            "thermals": [{"label":"cpu","celsius":48.5},{"label":"broken","celsius":999}],
            "system": {"hostname":"fixture","os":"Ubuntu","kernel":"4.19","arch":"aarch64"}
        });
        let mut metrics = Metrics::default();
        let initial = metrics.push(first.clone()).unwrap();
        assert!(initial.cpu.is_none());
        assert!(initial.cores.iter().all(|core| core.usage.is_none()));

        first["clock"] = json!(3.);
        first["cpuTotal"] = json!(1200);
        first["cpuIdle"] = json!(650);
        first["cpuCores"] =
            json!([{"id":1,"total":600,"idle":300},{"id":0,"total":600,"idle":350}]);
        first["network"] = json!([{"name":"wlan0","received":10,"sent":400},{"name":"eth0","received":700,"sent":140}]);
        let reading = metrics.push(first).unwrap();
        assert_eq!(reading.cpu, Some(75.));
        assert_eq!(
            reading
                .cores
                .iter()
                .map(|core| (core.id, core.usage))
                .collect::<Vec<_>>(),
            [(1, Some(100.)), (0, Some(50.))]
        );
        assert_eq!(reading.cpu_freq_mhz, Some(1800.));
        assert_eq!(reading.memory, 75.);
        assert_eq!(reading.memory_cached, Some(512));
        assert_eq!(reading.swap.unwrap().percent(), 75.);
        assert_eq!(reading.received, Some(100.));
        assert_eq!(reading.sent, Some(70.));
        assert_eq!(reading.networks[0].rx_rate, None);
        assert_eq!(reading.networks[0].tx_rate, Some(50.));
        assert_eq!(
            reading
                .storage
                .iter()
                .map(StorageReading::tone)
                .collect::<Vec<_>>(),
            [0, 2, 3]
        );
        assert_eq!(reading.storage[0].available, 200);
        assert_eq!(reading.thermals.len(), 1);
        assert_eq!(reading.system.unwrap().arch.as_deref(), Some("aarch64"));
        assert_eq!(reading.round_trip_ms, Some(14.));
    }

    #[test]
    fn absent_optional_fields_and_reconnects_after_a_failure_never_fabricate_values() {
        let mut metrics = Metrics::default();
        let mut sample = json!({"clock":100., "generation":1,"memoryTotal":100,
            "memoryAvailable":80,"cpuCores":null,"storage":null,"thermals":null});
        let reading = metrics.push(sample.clone()).unwrap();
        assert_eq!(reading.memory_bytes.used, 20);
        assert!(reading.cpu.is_none() && reading.swap.is_none() && reading.system.is_none());
        assert!(
            reading.storage.is_empty() && reading.thermals.is_empty() && reading.cores.is_empty()
        );
        metrics.failed();
        sample["clock"] = json!(5.);
        sample["generation"] = json!(2);
        assert!(metrics.push(sample).is_some());
        assert_eq!(metrics.path(true), "M100.00,8.00 ");
    }
}
