//! Native arena presentation. Missing measurements never become zero samples.
use serde_json::Value;
use std::collections::VecDeque;
#[derive(Default)]
pub struct Metrics {
    previous: Option<Value>,
    points: VecDeque<(f64, Option<f64>, Option<f64>)>,
}
pub struct Reading {
    pub cpu: Option<f64>,
    pub memory: f64,
    pub received: Option<f64>,
    pub sent: Option<f64>,
}
impl Metrics {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn failed(&mut self) {
        self.previous = None;
        if let Some(p) = self.points.back() {
            self.points.push_back((p.0 + 0.001, None, None));
        }
    }
    pub fn push(&mut self, v: Value) -> Option<Reading> {
        let clock = v["clock"].as_f64()?;
        let total = v["memoryTotal"].as_f64()?;
        let free = v["memoryAvailable"].as_f64()?;
        if !clock.is_finite() || total <= 0. || free < 0. || free > total {
            return None;
        }
        let mut r = Reading {
            cpu: None,
            memory: 100. * (total - free) / total,
            received: None,
            sent: None,
        };
        if let Some(p) = &self.previous {
            let elapsed = clock - p["clock"].as_f64().unwrap_or(clock);
            if v["generation"] == p["generation"] && elapsed > 0. && elapsed < 15. {
                let cpu = v["cpuTotal"].as_f64()? - p["cpuTotal"].as_f64()?;
                let idle = v["cpuIdle"].as_f64()? - p["cpuIdle"].as_f64()?;
                if cpu > 0. && idle >= 0. && idle <= cpu {
                    r.cpu = Some(100. * (cpu - idle) / cpu);
                }
                let mut received = 0.;
                let mut sent = 0.;
                let mut count = 0;
                for n in v["network"].as_array().into_iter().flatten() {
                    if let Some(old) = p["network"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|old| old["name"] == n["name"])
                    {
                        if let (Some(rx), Some(tx), Some(a), Some(b)) = (
                            n["received"].as_f64(),
                            n["sent"].as_f64(),
                            old["received"].as_f64(),
                            old["sent"].as_f64(),
                        ) {
                            if rx >= a && tx >= b {
                                received += (rx - a) / elapsed;
                                sent += (tx - b) / elapsed;
                                count += 1;
                            }
                        }
                    }
                }
                if count > 0 {
                    r.received = Some(received);
                    r.sent = Some(sent);
                }
            } else {
                self.points.clear();
            }
        }
        self.points.push_back((clock, r.cpu, Some(r.memory)));
        while self.points.front().is_some_and(|p| clock - p.0 > 60.) {
            self.points.pop_front();
        }
        self.previous = Some(v);
        Some(r)
    }
    pub fn path(&self, memory: bool) -> String {
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
                    (100. - (end - t) / 60. * 100.).clamp(0., 100.),
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
pub fn pose_live(value: &Value, elapsed_ms: f64) -> bool {
    value["phase"] == "live"
        && value["ageMs"]
            .as_f64()
            .is_some_and(|age| age + elapsed_ms <= 2000.)
}
#[cfg(test)]
mod tests {
    use super::*;
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
        m.push(sample(5., 300, 100));
        assert_eq!(m.path(true).matches('M').count(), 2);
        m.push(sample(70., 400, 150));
        assert!(m.points.iter().all(|p| p.0 >= 10.));
    }
    #[test]
    fn a_live_pose_expires_locally_without_a_new_packet() {
        let v = json!({"phase":"live","ageMs":100});
        assert!(pose_live(&v, 1800.));
        assert!(!pose_live(&v, 1901.));
        assert!(!pose_live(&json!({"phase":"unavailable"}), 0.));
    }
}
