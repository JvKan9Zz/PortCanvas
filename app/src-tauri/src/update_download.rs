use anyhow::{Context, Result};
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::{
    header::{ACCEPT_ENCODING, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE},
    Client, StatusCode,
};
use tauri_plugin_updater::Update;

const CONNECTIONS: usize = 32;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
fn ranges(size: u64) -> Vec<(u64, u64)> {
    let count = CONNECTIONS.min(size as usize) as u64;
    (0..count)
        .map(|index| (size * index / count, size * (index + 1) / count - 1))
        .collect()
}
fn content_range(text: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = text.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}
pub async fn download(
    update: &Update,
    progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
    verify: impl FnOnce(),
) -> Result<Vec<u8>> {
    let mut builder = Client::builder()
        .user_agent(concat!("PortCanvas/", env!("CARGO_PKG_VERSION")))
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
    let probe = client
        .get(update.download_url.clone())
        .header(RANGE, "bytes=0-0")
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await
        .context("无法连接更新下载服务。")?;
    let range = probe
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(content_range);
    if probe.status() != StatusCode::PARTIAL_CONTENT
        || !matches!(range,Some((0,0,size)) if size>=CONNECTIONS as u64)
    {
        let response = if probe.status() == StatusCode::OK {
            probe
        } else {
            drop(probe);
            client
                .get(update.download_url.clone())
                .header(ACCEPT_ENCODING, "identity")
                .send()
                .await?
        };
        let response = response.error_for_status()?;
        let total = response.content_length();
        anyhow::ensure!(
            total.is_none_or(|size| size <= MAX_PACKAGE_BYTES),
            "更新包超过 256 MiB 限制。"
        );
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        progress(0, total, 1);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            anyhow::ensure!(
                bytes.len() as u64 + chunk.len() as u64 <= MAX_PACKAGE_BYTES,
                "更新包超过 256 MiB 限制。"
            );
            bytes.extend_from_slice(&chunk);
            progress(chunk.len(), total, 1);
        }
        verify();
        update.verify(&bytes).context("更新文件签名校验失败。")?;
        return Ok(bytes);
    }
    let size = range.unwrap().2;
    anyhow::ensure!(size <= MAX_PACKAGE_BYTES, "更新包超过 256 MiB 限制。");
    let validator = probe
        .headers()
        .get(ETAG)
        .filter(|value| !value.as_bytes().starts_with(b"W/"))
        .or_else(|| probe.headers().get(LAST_MODIFIED))
        .cloned();
    drop(probe);
    let capacity = usize::try_from(size).context("更新文件大小超出当前平台限制。")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .context("没有足够内存下载更新。")?;
    bytes.resize(capacity, 0);
    progress(0, Some(size), CONNECTIONS);
    let mut requests = FuturesUnordered::new();
    for (start, end) in ranges(size) {
        let client = &client;
        let validator = validator.clone();
        let progress = &progress;
        requests.push(async move {
            let mut request = client
                .get(update.download_url.clone())
                .header(RANGE, format!("bytes={start}-{end}"))
                .header(ACCEPT_ENCODING, "identity");
            if let Some(validator) = validator {
                request = request.header(IF_RANGE, validator);
            }
            let response = request.send().await.context("更新分段下载失败。")?;
            anyhow::ensure!(
                response.status() == StatusCode::PARTIAL_CONTENT,
                "更新服务器未返回请求的分段，请重新下载。"
            );
            let actual = response
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|value| value.to_str().ok())
                .and_then(content_range);
            anyhow::ensure!(
                actual == Some((start, end, size)),
                "更新分段范围不匹配，请重新下载。"
            );
            let mut part = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                anyhow::ensure!(
                    part.len() as u64 + chunk.len() as u64 <= end - start + 1,
                    "更新分段长度不匹配。"
                );
                part.extend_from_slice(&chunk);
                progress(chunk.len(), Some(size), CONNECTIONS);
            }
            anyhow::ensure!(part.len() as u64 == end - start + 1, "更新分段下载不完整。");
            Ok::<_, anyhow::Error>((start, part))
        });
    }
    while let Some(result) = requests.next().await {
        let (start, part) = result?;
        bytes[start as usize..start as usize + part.len()].copy_from_slice(&part);
    }
    verify();
    update.verify(&bytes).context("更新文件签名校验失败。")?;
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partitions_cover_every_byte_once_including_uneven_files() {
        for size in [1, 31, 32, 33, 1048579, 16000000] {
            let parts = ranges(size);
            assert_eq!(parts.len(), CONNECTIONS.min(size as usize));
            let mut next = 0;
            for (start, end) in parts {
                assert_eq!(start, next);
                assert!(end >= start);
                next = end + 1;
            }
            assert_eq!(next, size);
        }
    }
    #[test]
    fn only_exact_byte_ranges_are_accepted() {
        assert_eq!(content_range("bytes 0-0/42"), Some((0, 0, 42)));
        for bad in ["items 0-0/42", "bytes */42", "bytes 0-0/*", "bytes 0-x/42"] {
            assert!(content_range(bad).is_none());
        }
    }
}
