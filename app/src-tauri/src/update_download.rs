use anyhow::{Context, Result};
use tauri_plugin_updater::Update;
use portcanvas_core::download::{client_builder, fetch};

pub async fn download(
    update: &Update,
    progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
    verify: impl FnOnce(),
) -> Result<Vec<u8>> {
    let mut builder = client_builder()
        .default_headers(update.headers.clone());
    if let Some(timeout) = update.timeout {
        builder = builder.timeout(timeout);
    }
    if update.no_proxy {
        builder = builder.no_proxy();
    } else if let Some(proxy) = &update.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy.as_str())?);
    }
    let client = builder.build()?;
    let bytes = fetch(&client, &update.download_url, progress).await?;
    verify();
    update.verify(&bytes).context("更新文件签名校验失败。")?;
    Ok(bytes)
}
