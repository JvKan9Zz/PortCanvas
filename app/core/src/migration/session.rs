use super::{Plan, RestorePlan};
use crate::{logs::Settings, services::Services};
use anyhow::{ensure, Result};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
};
pub struct MigrationSession {
    services: Arc<Services>,
}
impl Drop for MigrationSession {
    fn drop(&mut self) {
        self.services.migrating.store(false, Ordering::SeqCst);
    }
}
impl MigrationSession {
    pub fn begin(services: Arc<Services>) -> Result<Self> {
        ensure!(
            services
                .migrating
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "资料迁移正在进行，请稍后重试。"
        );
        let owner = Self { services };
        owner.check_idle()?;
        Ok(owner)
    }
    pub fn check_idle(&self) -> Result<()> {
        ensure!(
            !self.services.quitting.load(Ordering::SeqCst)
                && !matches!(
                    self.services.devices.snapshot()["ssh"].as_str(),
                    Some("connected" | "connecting")
                )
                && self.services.files.snapshot()["busy"] != true,
            "请先断开设备并等待任务结束，再迁移资料。"
        );
        self.services.logs.can_change_root()
    }
    async fn run(
        self,
        settings: Settings,
        operation: impl FnOnce() -> Result<Value> + Send + 'static,
    ) -> Result<Value> {
        self.check_idle()?;
        let previous = self.services.logs.settings();
        self.services.logs.replace_settings(settings, false).await?;
        let (owner, result) = tokio::task::spawn_blocking(move || {
            let profiles = self.services.profiles.clone();
            let result = (|| {
                let _lock = profiles.lock().unwrap();
                self.check_idle()?;
                operation()
            })();
            (self, result)
        })
        .await?;
        if let Err(error) = result {
            if !owner.services.quitting.load(Ordering::SeqCst) {
                if let Err(runtime) = owner.services.logs.replace_settings(previous, false).await {
                    anyhow::bail!("{error}；日志运行态恢复失败：{runtime}");
                }
            }
            return Err(error);
        }
        result
    }
    pub async fn apply(self, plan: Plan) -> Result<Value> {
        ensure!(
            self.services.profiles.lock().unwrap().directory == plan.target,
            "迁移目标与当前资料目录不一致。"
        );
        let settings = plan.settings.clone();
        self.run(settings, move || plan.apply()).await
    }
    pub async fn restore(self, backup: PathBuf) -> Result<Value> {
        self.check_idle()?;
        let profiles = self.services.profiles.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let store = profiles.lock().unwrap();
            RestorePlan::prepare(store.directory.clone(), backup)
        })
        .await??;
        let settings = plan.settings(self.services.default_log_root.clone())?;
        self.run(settings, move || plan.apply()).await
    }
}
