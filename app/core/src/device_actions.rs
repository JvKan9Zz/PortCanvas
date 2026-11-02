//! Device actions shared by UI adapters. Confirmation never grants a different connection.
use crate::services::Services;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{future::Future, sync::atomic::Ordering};
struct RebootGuard<'a>(&'a Services);
impl Drop for RebootGuard<'_> {
    fn drop(&mut self) {
        self.0
            .devices
            .update(self.0.devices.generation(), json!({"rebooting":false}));
        self.0.rebooting.store(false, Ordering::SeqCst);
    }
}
impl Services {
    pub async fn reboot<F: Future<Output = Result<bool>>>(
        &self,
        confirm: impl FnOnce(String) -> F,
    ) -> Result<Value> {
        ensure!(
            self.rebooting
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "重启请求正在处理中。"
        );
        let _guard = RebootGuard(self);
        let available = || -> Result<()> {
            ensure!(!self.quitting.load(Ordering::SeqCst), "软件正在退出。");
            ensure!(
                !self.migrating.load(Ordering::SeqCst) && self.files.snapshot()["busy"] != true,
                "请先完成或取消文件操作后再重启设备。"
            );
            Ok(())
        };
        available()?;
        let (generation, connection) = self.devices.connection_with_generation()?;
        let target = format!(
            "{}\n{}@{}",
            connection.profile.name,
            connection.profile.username,
            connection.profile.address()
        );
        if !confirm(target).await? {
            return Ok(json!({"canceled":true}));
        }
        let current = || -> Result<()> {
            ensure!(
                generation == self.devices.generation() && !connection.cancel.is_cancelled(),
                "设备连接已变化，请重新确认重启目标。"
            );
            available()
        };
        current()?;
        self.devices.update(generation, json!({"rebooting":true}));
        tokio::join!(
            self.desktop.stop(None),
            self.arena.stop(None),
            self.pose.stop(None)
        );
        current()?;
        crate::desktop::schedule_reboot(&connection).await?;
        if generation == self.devices.generation() {
            self.devices.disconnect().await;
        }
        Ok(json!({"requested":true}))
    }
}
