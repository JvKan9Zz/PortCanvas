//! UI-independent profile actions and SSH identity policy.
use crate::{profiles::Profile, services::Services, ssh::HostCheck};
use anyhow::{ensure, Result};
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[derive(Clone, Debug)]
pub struct HostIdentity {
    pub address: String,
    pub fingerprint: String,
}
pub type HostConfirmation =
    Arc<dyn Fn(HostIdentity, CancellationToken) -> BoxFuture<'static, Result<bool>> + Send + Sync>;

impl Services {
    pub fn save_profile(&self, profile: Profile, secret: &str) -> Result<Profile> {
        ensure!(
            !matches!(
                self.devices.snapshot()["ssh"].as_str(),
                Some("connected" | "connecting")
            ),
            "请先断开设备再修改配置。"
        );
        self.profiles.lock().unwrap().save(profile, secret)
    }
    /// Caller confirms removal before invoking this action.
    pub async fn remove_profile(&self, id: &str) -> Result<()> {
        if self.devices.snapshot()["deviceId"] == id {
            self.devices.disconnect().await;
        }
        let store = self.profiles.lock().unwrap();
        let mut data = store.read()?;
        let removed = data.devices.iter().find(|p| p.id == id).cloned();
        data.devices.retain(|p| p.id != id);
        if let Some(removed) = removed {
            if !data
                .devices
                .iter()
                .any(|p| p.address() == removed.address())
            {
                data.host_keys.remove(&removed.address());
            }
        }
        store.write(&data)
    }
    pub async fn connect(
        &self,
        profile: Profile,
        supplied: Zeroizing<String>,
        confirm: HostConfirmation,
    ) -> Result<Value> {
        let mut profile = profile.validate()?;
        profile.encrypted_secret = None;
        self.discovery.cancel();
        let password = self
            .profiles
            .lock()
            .unwrap()
            .connection_secret(&profile, supplied)?;
        let store = self.profiles.clone();
        let address = profile.address();
        self.devices
            .connect(profile, password, move |cancel| {
                let callback: HostCheck = Arc::new(move |fingerprint| {
                    let store = store.clone();
                    let address = address.clone();
                    let cancel = cancel.clone();
                    let confirm = confirm.clone();
                    Box::pin(async move {
                        let known = store
                            .lock()
                            .unwrap()
                            .read()?
                            .host_keys
                            .get(&address)
                            .cloned();
                        if let Some(known) = known {
                            ensure!(
                                known == fingerprint,
                                "主机身份已变化，已停止连接。请核实设备身份，不会自动信任新密钥。"
                            );
                            return Ok(!cancel.is_cancelled());
                        }
                        let accepted = confirm(
                            HostIdentity {
                                address: address.clone(),
                                fingerprint: fingerprint.clone(),
                            },
                            cancel.clone(),
                        )
                        .await?;
                        if !accepted || cancel.is_cancelled() {
                            return Ok(false);
                        }
                        store.lock().unwrap().trust(&address, &fingerprint)?;
                        Ok(true)
                    })
                });
                callback
            })
            .await
    }
    pub async fn disconnect(&self) -> Value {
        tokio::join!(self.arena.stop(None), self.pose.stop(None));
        self.devices.disconnect().await;
        self.desktop.stop(None).await;
        self.devices.snapshot()
    }
}
