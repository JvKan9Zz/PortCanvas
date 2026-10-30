use anyhow::{Context, Result};
use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::{
    header::{ACCEPT_ENCODING, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE},
    Client, StatusCode,
};

const CONNECTIONS: usize = 32;
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
pub fn client_builder() -> reqwest::ClientBuilder {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder().user_agent(concat!("PortCanvas/", env!("CARGO_PKG_VERSION")))
}
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
// Shared range transport; each caller keeps its own package authentication.
pub async fn fetch(
    client: &Client,
    url: &reqwest::Url,
    progress: impl Fn(usize, Option<u64>, usize) + Send + Sync,
) -> Result<Vec<u8>> {
    let probe = client
        .get(url.clone())
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
                .get(url.clone())
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
                .get(url.clone())
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
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn fixture(mode: u8) -> (reqwest::Url, tokio::task::JoinHandle<()>, Vec<u8>) {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url=format!("http://{}/package",listener.local_addr().unwrap()).parse().unwrap();
        let bytes=(0..65539).map(|i|(i%251) as u8).collect::<Vec<_>>();
        let body=Arc::new(bytes.clone());
        let task=tokio::spawn(async move {
            loop {
                let (mut socket,_)=listener.accept().await.unwrap();let body=body.clone();
                tokio::spawn(async move {
                    let mut header=Vec::new();let mut byte=[0];
                    while !header.ends_with(b"\r\n\r\n") { if socket.read_exact(&mut byte).await.is_err(){return;}header.push(byte[0]); }
                    let text=String::from_utf8(header).unwrap().to_lowercase();
                    let range=text.lines().find_map(|line|line.strip_prefix("range: bytes="))
                        .and_then(|value|value.split_once('-')).map(|(a,b)|(a.parse::<usize>().unwrap(),b.parse::<usize>().unwrap()));
                    let (response,content)=if mode>0 && range.is_some() {
                        let (start,end)=range.unwrap();
                        let reported=if mode==2 && end>0 {start+1}else{start};
                        (format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {reported}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len(),end-start+1),&body[start..=end])
                    }else{(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()),&body[..])};
                    let _=socket.write_all(response.as_bytes()).await;let _=socket.write_all(content).await;
                });
            }
        });
        (url,task,bytes)
    }
    #[tokio::test]
    async fn shared_download_reports_real_range_and_single_connection_bytes() {
        for mode in [0,1] {
            let (url,server,expected)=fixture(mode).await;
            let progress=Mutex::new((0usize,0usize));
            let result=fetch(&client_builder().no_proxy().build().unwrap(),&url,|bytes,total,connections|{
                assert_eq!(total,Some(65539));let mut progress=progress.lock().unwrap();progress.0+=bytes;progress.1=connections;
            }).await;
            server.abort();
            assert_eq!(result.unwrap(),expected);
            assert_eq!(*progress.lock().unwrap(),(expected.len(),if mode==0{1}else{32}));
        }
    }
    #[tokio::test]
    async fn shared_download_rejects_changed_range_boundaries() {
        let (url,server,_)=fixture(2).await;
        let result=fetch(&client_builder().no_proxy().build().unwrap(),&url,|_,_,_|{}).await;
        server.abort();assert!(result.is_err());
    }
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
