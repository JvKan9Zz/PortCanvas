use serde::{Deserialize,Serialize};
use serde_json::{json,Value};

#[derive(Clone,Copy,Default,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="lowercase")]
pub enum DesktopMode {#[default] Smooth,Clear,Economy}
impl DesktopMode {
    pub fn tuning(self)->Value {
        let(width,height,wait,interval,quality)=match self {
            Self::Smooth=>(0,0,8,0.025,5),
            Self::Clear=>(0,0,12,0.04,8),
            Self::Economy=>(960,540,12,0.04,4),
        };
        json!({"maxWidth":width,"maxHeight":height,"wait":wait,"defer":1,"frameInterval":interval,"quality":quality,"compression":1})
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn modes_are_bounded_and_clear_mode_keeps_source_pixels(){
        assert_eq!(DesktopMode::Clear.tuning()["maxWidth"],0);
        assert_eq!(DesktopMode::Smooth.tuning()["maxWidth"],0);
        assert!(DesktopMode::Smooth.tuning()["frameInterval"].as_f64().unwrap()<DesktopMode::Economy.tuning()["frameInterval"].as_f64().unwrap());
        for mode in [DesktopMode::Smooth,DesktopMode::Clear,DesktopMode::Economy]{assert_eq!(mode.tuning()["compression"],1);assert!(mode.tuning()["quality"].as_u64().unwrap()<=9);}
        assert!(serde_json::from_str::<DesktopMode>("\"invalid\"").is_err());
    }
}
