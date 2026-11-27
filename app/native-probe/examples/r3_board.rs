//! Explicit, isolated real-device diagnostics over the existing trusted SSH service.
use anyhow::{ensure, Context, Result};
use portcanvas_core::{desktop::python, services::{Paths,Services}};
use std::{path::PathBuf,sync::Arc,time::Duration};
#[tokio::main]
async fn main()->Result<()> {
    let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?).canonicalize()?;
    let source=std::fs::read_to_string(root.join("command.py"))?;
    let services=Services::new(Paths{plugins:None,data:root.join("data"),documents:root.clone(),local_files:root.join("local-files"),portable:true},Arc::new(|_,_|{}),Some("r3-diagnostics".into()))?;
    let host=std::env::var("WCS_BOARD_HOST").context("explicit WCS_BOARD_HOST required")?;
    let profile=services.profiles.lock().unwrap().read()?.devices.into_iter().find(|p|p.host==host).context("trusted saved profile required")?;
    ensure!(profile.remember_password,"saved credential required; do not export it");
    let result=async {
        services.connect(profile,zeroize::Zeroizing::new(String::new()),Arc::new(|_,_|Box::pin(async{Ok(false)}))).await?;
        let command=python(&source,serde_json::json!([]))?;
        let (text,status)=services.devices.connection()?.exec(&command,256*1024,Duration::from_secs(90)).await?;
        std::fs::write(root.join("result.txt"),text.as_bytes())?;
        ensure!(status==0,"diagnostic returned {status}");
        println!("Saved bounded board response ({} bytes)",text.len());Ok::<(),anyhow::Error>(())
    }.await;
    services.shutdown().await;result
}
