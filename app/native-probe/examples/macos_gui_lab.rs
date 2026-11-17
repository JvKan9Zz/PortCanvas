//! Controlled loopback peer for real native-window acceptance; never executes shell commands.
#[path="../tests/support/terminal_fixture.rs"] mod support;
use anyhow::{ensure,Result};
use std::{path::PathBuf,time::Duration};
#[tokio::main]
async fn main()->Result<()> {
    let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").ok_or_else(||anyhow::anyhow!("isolated WCS_TEST_ROOT required"))?);
    ensure!(root.is_absolute(),"absolute test root required");
    let data=root.join("gui-lab-data");ensure!(!data.exists(),"GUI lab data directory already exists");
    let remote=root.join("gui-lab-remote");std::fs::create_dir_all(&remote)?;std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(remote.join("中文目录"))?;
    std::fs::write(remote.join("hello-中文.txt"),"controlled SFTP sample：全角；。\n")?;
    let local=root.join("local-files");std::fs::create_dir_all(&local)?;std::fs::write(local.join("upload-中文.txt"),"owned native GUI lab fixture\n")?;
    let fixture=support::Fixture::with_files(remote).await?;
    let mut profile=fixture.profile.clone();profile.remember_password=false;profile.name="受控回环 · 非实体设备".into();
    std::fs::write(data.join("devices.json"),serde_json::to_vec_pretty(&serde_json::json!({"devices":[profile],"hostKeys":{}}))?)?;
    std::fs::write(root.join("gui-lab-peer.json"),serde_json::to_vec_pretty(&serde_json::json!({"host":"127.0.0.1","port":fixture.profile.port,"fingerprint":fixture.fingerprint,"data":data,"simulation":true,"password":"synthetic-only"}))?)?;
    println!("Controlled peer ready on 127.0.0.1:{}; synthetic-only; no shell execution",fixture.profile.port);
    loop {
        let inputs=fixture.counts.input.lock().unwrap().iter().map(|(key,value)|(key.clone(),String::from_utf8_lossy(value).into_owned())).collect::<std::collections::BTreeMap<_,_>>();
        std::fs::write(root.join("gui-lab-peer-report.json"),serde_json::to_vec_pretty(&serde_json::json!({"simulation":true,"counts":fixture.report(),"input":inputs}))?)?;
        if root.join("gui-lab-stop").exists(){break;}
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}
