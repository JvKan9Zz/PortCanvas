//! Runtime display constraints; this module never writes user preferences.
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub fn is_low_battery(present: bool, on_battery: bool, percent: Option<i32>) -> bool {
    present && on_battery && percent.is_some_and(|p| (0..=20).contains(&p))
}

#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct SystemEffects {
    pub high_contrast: bool,
    pub transparency_off: bool,
    pub animations_off: bool,
    pub energy_saver: bool,
    pub low_battery: bool,
    pub unavailable: bool,
}
impl SystemEffects {
    pub fn solid(self) -> bool {
        self.high_contrast || self.transparency_off || self.animations_off || self.unavailable
    }
    pub fn effective(self, preferred: i32) -> i32 {
        if self.solid() || self.energy_saver || self.low_battery {
            0
        } else {
            preferred.clamp(0, 2)
        }
    }
    pub fn notice(self) -> &'static str {
        if self.high_contrast {
            "Windows 高对比度已开启，当前显示不透明基础材质。"
        } else if self.transparency_off {
            "Windows 透明效果已关闭，当前显示不透明基础材质。"
        } else if self.animations_off {
            "Windows 动画效果已关闭，当前显示不透明基础材质。"
        } else if self.unavailable {
            "系统效果状态不可用，当前显示不透明基础材质。"
        } else if self.energy_saver || self.low_battery {
            "节能或低电量状态：当前使用轻量材质。"
        } else {
            ""
        }
    }
}

/// Bounded UI event-loop lateness window, fed only during visible material activity.
/// Skip a two-second warmup; suggest after 8/12 ticks exceed 50ms lateness.
#[derive(Default)]
pub struct Responsiveness {
    started_ms: Option<u64>,
    samples: VecDeque<bool>,
}
impl Responsiveness {
    pub fn reset(&mut self) {
        self.started_ms = None;
        self.samples.clear();
    }
    pub fn sample(&mut self, now_ms: u64, late_ms: u64) -> bool {
        let start = *self.started_ms.get_or_insert(now_ms);
        if late_ms > 1000 {
            self.reset();
            return false;
        }
        if now_ms.saturating_sub(start) < 2000 {
            return false;
        }
        self.samples.push_back(late_ms >= 50);
        if self.samples.len() > 12 {
            self.samples.pop_front();
        }
        self.samples.len() == 12 && self.samples.iter().filter(|s| **s).count() >= 8
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_battery_requires_known_unplugged_battery_and_valid_percentage() {
        assert!(is_low_battery(true, true, Some(0)));
        assert!(is_low_battery(true, true, Some(20)));
        for value in [None, Some(-1), Some(21), Some(255)] {
            assert!(!is_low_battery(true, true, value));
        }
        assert!(!is_low_battery(false, true, Some(0)));
        assert!(!is_low_battery(true, false, Some(10)));
    }
    #[test]
    fn constraints_do_not_mutate_preferred_quality() {
        for preferred in 0..=2 {
            assert_eq!(SystemEffects::default().effective(preferred), preferred);
            for p in [
                SystemEffects {
                    high_contrast: true,
                    ..Default::default()
                },
                SystemEffects {
                    transparency_off: true,
                    ..Default::default()
                },
                SystemEffects {
                    animations_off: true,
                    ..Default::default()
                },
                SystemEffects {
                    unavailable: true,
                    ..Default::default()
                },
            ] {
                assert!(p.solid());
                assert_eq!(p.effective(preferred), 0);
                assert!(!p.notice().is_empty());
            }
            let p = SystemEffects {
                energy_saver: true,
                ..Default::default()
            };
            assert!(!p.solid());
            assert_eq!(p.effective(preferred), 0);
        }
    }
    #[test]
    fn isolated_stalls_warmup_and_native_dialog_gaps_do_not_suggest() {
        let mut m = Responsiveness::default();
        for n in 0..80 {
            assert!(!m.sample(n * 250, if n % 5 == 0 { 100 } else { 0 }));
        }
        m.reset();
        for n in 0..8 {
            assert!(!m.sample(n * 250, 100));
        }
        assert!(!m.sample(2250, 5000));
        for n in 10..18 {
            assert!(!m.sample(n * 250, 100));
        }
    }
    #[test]
    fn sustained_delay_qualifies_and_reset_requires_new_warmup() {
        let mut m = Responsiveness::default();
        for n in 0..18 {
            assert!(!m.sample(n * 300, 60));
        }
        assert!(m.sample(5400, 60));
        m.reset();
        assert!(!m.sample(6000, 60));
        for n in 1..25 {
            assert!(!m.sample(6000 + n * 250, 0));
        }
    }
}
