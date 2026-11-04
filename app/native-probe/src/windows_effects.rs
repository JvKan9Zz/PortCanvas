//! Read-only WinRT policy notifications. No registry edits, global hooks or polling.
use crate::effects_policy::SystemEffects;
use windows::{
    core::IInspectable,
    Foundation::{EventHandler, TypedEventHandler},
    System::Power::{BatteryStatus, EnergySaverStatus, PowerManager, PowerSupplyStatus},
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_SINGLETHREADED},
    UI::ViewManagement::{
        AccessibilitySettings, UISettings, UISettingsAnimationsEnabledChangedEventArgs,
    },
};

struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                RoUninitialize();
            }
        }
    }
}
pub struct WindowsEffects {
    settings: Option<UISettings>,
    accessibility: Option<AccessibilitySettings>,
    advanced: Option<i64>,
    animation: Option<i64>,
    contrast: Option<i64>,
    saver: Option<i64>,
    charge: Option<i64>,
    supply: Option<i64>,
    battery: Option<i64>,
    errors: Vec<String>,
    _apartment: Apartment,
}
impl WindowsEffects {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let apartment = Apartment(unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.is_ok());
        let wake = std::sync::Arc::new(wake);
        let mut errors = Vec::new();
        let mut token = |r: windows::core::Result<i64>| match r {
            Ok(t) => Some(t),
            Err(e) => {
                errors.push(e.to_string());
                None
            }
        };
        let settings = UISettings::new().ok();
        let accessibility = AccessibilitySettings::new().ok();
        let advanced = settings.as_ref().and_then(|s| {
            let w = wake.clone();
            token(s.AdvancedEffectsEnabledChanged(
                &TypedEventHandler::<UISettings, IInspectable>::new(move |_, _| {
                    w();
                    Ok(())
                }),
            ))
        });
        let animation = settings.as_ref().and_then(|s| {
            let w = wake.clone();
            token(s.AnimationsEnabledChanged(&TypedEventHandler::<
                UISettings,
                UISettingsAnimationsEnabledChangedEventArgs,
            >::new(move |_, _| {
                w();
                Ok(())
            })))
        });
        // HighContrastChanged is unavailable on the tested desktop backend (0x80070490).
        // Contrast themes change system colors; use the desktop-capable UISettings event.
        let contrast = settings.as_ref().and_then(|s| {
            let w = wake.clone();
            token(
                s.ColorValuesChanged(&TypedEventHandler::<UISettings, IInspectable>::new(
                    move |_, _| {
                        w();
                        Ok(())
                    },
                )),
            )
        });
        let handler = EventHandler::<IInspectable>::new(move |_, _| {
            wake();
            Ok(())
        });
        let saver = token(PowerManager::EnergySaverStatusChanged(&handler));
        let charge = token(PowerManager::RemainingChargePercentChanged(&handler));
        let supply = token(PowerManager::PowerSupplyStatusChanged(&handler));
        let battery = token(PowerManager::BatteryStatusChanged(&handler));
        Self {
            settings,
            accessibility,
            advanced,
            animation,
            contrast,
            saver,
            charge,
            supply,
            battery,
            errors,
            _apartment: apartment,
        }
    }
    pub fn read(&self) -> SystemEffects {
        let transparency = self
            .settings
            .as_ref()
            .and_then(|s| s.AdvancedEffectsEnabled().ok());
        let animations = self
            .settings
            .as_ref()
            .and_then(|s| s.AnimationsEnabled().ok());
        let contrast = self
            .accessibility
            .as_ref()
            .and_then(|s| s.HighContrast().ok());
        let percent = PowerManager::RemainingChargePercent().ok();
        let has_battery = PowerManager::BatteryStatus()
            .ok()
            .is_some_and(|s| s != BatteryStatus::NotPresent);
        let supplied = PowerManager::PowerSupplyStatus().ok();
        SystemEffects {
            high_contrast: contrast == Some(true),
            transparency_off: transparency == Some(false),
            animations_off: animations == Some(false),
            energy_saver: PowerManager::EnergySaverStatus().ok() == Some(EnergySaverStatus::On),
            low_battery: crate::effects_policy::is_low_battery(
                has_battery,
                supplied == Some(PowerSupplyStatus::NotPresent),
                percent,
            ),
            unavailable: transparency.is_none() || animations.is_none() || contrast.is_none(),
        }
    }
    pub fn report(&self) -> serde_json::Value {
        let subscriptions = [
            self.advanced,
            self.animation,
            self.contrast,
            self.saver,
            self.charge,
            self.supply,
            self.battery,
        ]
        .iter()
        .filter(|t| t.is_some())
        .count();
        serde_json::json!({"snapshot":self.read(),"subscriptions":subscriptions,"subscriptionErrors":self.errors,
            "listeners":{"advanced":self.advanced.is_some(),"animation":self.animation.is_some(),"contrast":self.contrast.is_some(),"saver":self.saver.is_some(),"charge":self.charge.is_some(),"supply":self.supply.is_some(),"battery":self.battery.is_some()}})
    }
}
impl WindowsEffects {
    pub fn close(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut check = |r: windows::core::Result<()>| {
            if let Err(e) = r {
                errors.push(e.to_string());
            }
        };
        if let (Some(s), Some(t)) = (&self.settings, self.advanced.take()) {
            check(s.RemoveAdvancedEffectsEnabledChanged(t));
        }
        if let (Some(s), Some(t)) = (&self.settings, self.animation.take()) {
            check(s.RemoveAnimationsEnabledChanged(t));
        }
        if let (Some(s), Some(t)) = (&self.settings, self.contrast.take()) {
            check(s.RemoveColorValuesChanged(t));
        }
        if let Some(t) = self.saver.take() {
            check(PowerManager::RemoveEnergySaverStatusChanged(t));
        }
        if let Some(t) = self.charge.take() {
            check(PowerManager::RemoveRemainingChargePercentChanged(t));
        }
        if let Some(t) = self.supply.take() {
            check(PowerManager::RemovePowerSupplyStatusChanged(t));
        }
        if let Some(t) = self.battery.take() {
            check(PowerManager::RemoveBatteryStatusChanged(t));
        }
        errors
    }
}
impl Drop for WindowsEffects {
    fn drop(&mut self) {
        for error in self.close() {
            eprintln!("Effects event unsubscribe: {error}");
        }
    }
}
