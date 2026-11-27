//! Native arena presentation.
pub use crate::monitor_model::{Metrics, Reading};
use serde_json::Value;

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
    #[test]
    fn a_live_pose_expires_locally_without_a_new_packet() {
        let v = json!({"phase":"live","ageMs":100});
        assert!(pose_live(&v, 1800.));
        assert!(!pose_live(&v, 1901.));
        assert!(!pose_live(&json!({"phase":"unavailable"}), 0.));
    }
}
