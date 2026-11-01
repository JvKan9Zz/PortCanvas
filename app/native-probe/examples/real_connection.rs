//! Explicit real-device acceptance runner. Requires an isolated copy of a trusted profile.
use anyhow::{ensure, Context, Result};
use portcanvas_core::{
    files::{Confirmation, Side},
    services::{Paths, Services},
    terminal::TerminalService,
};
use portcanvas_native_probe::{
    desktop_client::{self, Channels, State},
    terminal::Terminal,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

async fn until(mut predicate: impl FnMut() -> bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("acceptance observation timed out")
}
async fn attach(t: &TerminalService, buffer: Arc<Mutex<Terminal>>) -> Result<u64> {
    let ack = t.clone();
    let state = t
        .start(
            100,
            30,
            Arc::new(move |bytes| {
                let id = u64::from_le_bytes(bytes[..8].try_into()?);
                let seq = u64::from_le_bytes(bytes[8..16].try_into()?);
                buffer.lock().unwrap().feed(&bytes[16..]);
                ack.ack(id, seq);
                Ok(())
            }),
        )
        .await?;
    state["id"].as_u64().context("terminal id")
}
fn confirm(yes: bool) -> Confirmation {
    Arc::new(move |_| Box::pin(async move { Ok(yes) }))
}
async fn exercise(s: &Arc<Services>, root: &std::path::Path, report: &mut Value) -> Result<()> {
    let profile = s
        .profiles
        .lock()
        .unwrap()
        .read()?
        .devices
        .into_iter()
        .next()
        .context("one copied profile required")?;
    s.connect(
        profile.clone(),
        zeroize::Zeroizing::new(String::new()),
        Arc::new(|_, _| Box::pin(async { Ok(false) })),
    )
    .await?;
    report["ssh"] = json!(s.devices.snapshot()["ssh"]);
    let home = s.files.list(Side::Remote, String::new()).await?;
    let parent = home["path"].as_str().context("remote home")?.to_owned();
    let name = format!(".portcanvas-native-test-{}", uuid::Uuid::new_v4());
    let remote = s
        .files
        .mkdir_for_generation(
            Side::Remote,
            parent.clone(),
            name.clone(),
            Some(s.devices.generation()),
        )
        .await?;
    report["remoteTestDirectory"] = json!(remote);
    let result=async {
        std::fs::create_dir_all(root.join("local-files/中文 目录/空目录"))?;
        std::fs::create_dir_all(root.join("download"))?;
        let original="原生客户端真实SFTP测试🙂\n";
        std::fs::write(root.join("local-files/中文 目录/设置.txt"),original)?;
        let mut options=json!({"generation":s.devices.generation(),"direction":"upload","localPath":root.join("local-files"),"remotePath":remote,"names":["中文 目录"]});
        ensure!(s.files.transfer(serde_json::from_value(options.clone())?,confirm(true)).await?["phase"]=="completed");
        std::fs::write(root.join("local-files/中文 目录/设置.txt"),"更新内容\n")?;
        ensure!(s.files.transfer(serde_json::from_value(options.clone())?,confirm(false)).await?["phase"]=="canceled");
        options["direction"]=json!("download"); options["localPath"]=json!(root.join("download"));
        ensure!(s.files.transfer(serde_json::from_value(options)?,confirm(true)).await?["phase"]=="completed");
        ensure!(std::fs::read(root.join("download/中文 目录/设置.txt"))?==original.as_bytes());
        ensure!(root.join("download/中文 目录/空目录").is_dir());
        report["sftp"]=json!({"unicodeRecursiveRoundTrip":true,"emptyDirectory":true,"overwriteCanceled":true});
        std::fs::write(root.join("local-files/cancel.bin"),vec![73u8;8*1024*1024])?;
        for disconnect in [false,true] {
            let service=s.clone();
            let options=json!({"generation":s.devices.generation(),"direction":"upload","localPath":root.join("local-files"),"remotePath":remote,"names":["cancel.bin"],"rateLimit":1048576});
            let job=tokio::spawn(async move{service.files.transfer(serde_json::from_value(options)?,confirm(true)).await});
            until(||{let v=s.files.snapshot();v["busy"]==true&&v["phase"]=="transferring"&&v["bytes"].as_u64().unwrap_or(0)>0}).await?;
            if disconnect {
                s.disconnect().await;
            } else {s.files.cancel();}
            let result=tokio::time::timeout(Duration::from_secs(20),job).await???;
            ensure!(result["phase"]==if disconnect{"failed"}else{"canceled"});
            if disconnect {s.connect(profile.clone(),zeroize::Zeroizing::new(String::new()),Arc::new(|_,_|Box::pin(async{Ok(false)}))).await?;}
            let contents=s.files.list(Side::Remote,remote.clone()).await?;
            ensure!(contents["entries"].as_array().unwrap().iter().all(|e|e["name"]!="cancel.bin"&&!e["name"].as_str().unwrap().starts_with(".wcs-transfer-")));
        }
        report["sftp"]["cancelAndDisconnectCleanup"]=json!(true);
        let t=s.terminal.get("main",None)?;
        let buffer=Arc::new(Mutex::new(Terminal::new(100,30)));
        let terminal_result=async {
            let id=attach(&t,buffer.clone()).await?;
            report["terminalStart"]=t.snapshot();
            // Only synthetic output, no SDK/hardware or user programs.
            t.write(id,"printf '\\033[32mNATIVE_%s\\033[0m 中文🙂\\n' REAL; printf 'PID_%s\\n' $$\r".into()).await?;
            until(||buffer.lock().unwrap().text().contains("NATIVE_REAL 中文🙂")).await.context("terminal Unicode/ANSI output")?;
            let before=buffer.lock().unwrap().text();
            let pid=before.lines().find(|l|l.trim().starts_with("PID_")).context("shell pid")?.trim().to_owned();
            let completion=t.complete(id,"python".into(),true,false).await?;
            ensure!(!completion["items"].as_array().context("completion list")?.is_empty());
            t.resize(id,120,35).await?;
            t.write(id,"printf 'SIZE_'; stty size\r".into()).await?;
            until(||buffer.lock().unwrap().text().contains("SIZE_35 120")).await.context("actual PTY size")?;
            t.stop();
            let id=attach(&t,buffer.clone()).await?;
            t.write(id,"printf 'RESUME_%s\\n' $$; stty size\r".into()).await?;
            let expected=pid.replacen("PID_","RESUME_",1);
            until(||buffer.lock().unwrap().text().contains(&expected)).await.context("terminal resumed PID")?;
            s.disconnect().await;
            s.connect(profile.clone(),zeroize::Zeroizing::new(String::new()),Arc::new(|_,_|Box::pin(async{Ok(false)}))).await?;
            let id=attach(&t,buffer.clone()).await?;
            t.write(id,"printf 'RECONNECT_%s\\n' $$\r".into()).await?;
            let expected=pid.replacen("PID_","RECONNECT_",1);
            until(||buffer.lock().unwrap().text().contains(&expected)).await.context("SSH reconnect same PID")?;
            report["terminal"]=json!({"realTmux":true,"unicodeAnsi":true,"completion":true,"detachResumeSamePid":true,"sshReconnectSamePid":true,"resizeObserved":true});
            Ok::<_,anyhow::Error>(())
        }.await;
        if terminal_result.is_err() { std::fs::write(root.join("synthetic-terminal-output.txt"),buffer.lock().unwrap().text())?; }
        let cleanup=t.reset(true).await;
        terminal_result?; cleanup?;
        let (input,inputs)=mpsc::channel(64);
        let (motion,motions)=watch::channel(Default::default());
        let (frames,frame_rx)=watch::channel(None);
        let (clipboard,_)=watch::channel(None);
        let ready=Arc::new(Mutex::new(None::<State>));let seen=ready.clone();
        let cancel=CancellationToken::new();let token=cancel.clone();
        let service=s.clone();let generation=s.devices.generation();
        let task=tokio::spawn(async move{desktop_client::run(service,generation,Default::default(),Channels{inputs,motion:motions,frames,clipboard,state:Arc::new(move|v|*seen.lock().unwrap()=Some(v))},token).await});
        let observed=until(||ready.lock().unwrap().is_some_and(|v|v.ready)&&frame_rx.borrow().is_some()).await;
        if observed.is_ok(){let frame=frame_rx.borrow().clone().unwrap();report["desktop"]=json!({"realX11vnc":true,"completeFrame":true,"width":frame.width,"height":frame.height});}
        cancel.cancel();let stopped=tokio::time::timeout(Duration::from_secs(12),task).await;
        drop(input);drop(motion);observed?;
        if let Err(error)=stopped?? {ensure!(error.to_string()=="桌面连接已取消。","{error}");}
        ensure!(s.devices.snapshot()["ssh"]=="connected");
        report["parentSshAfterDesktop"]=json!(true);
        Ok::<_,anyhow::Error>(())
    }.await;
    let cleanup = s
        .files
        .remove_remote(
            serde_json::from_value(
                json!({"parent":parent,"names":[name],"generation":s.devices.generation()}),
            )?,
            confirm(true),
        )
        .await;
    report["remoteDirectoryRemoved"] =
        json!(cleanup.as_ref().is_ok_and(|v| v["phase"] == "completed"));
    result?;
    cleanup?;
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("WCS_TEST_ROOT required")?);
    ensure!(
        root.is_absolute() && root.join("data/devices.json").is_file(),
        "isolated trusted profile required"
    );
    let s = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.clone(),
            local_files: root.join("local-files"),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some(format!("native-acceptance-{}", uuid::Uuid::new_v4())),
    )?;
    let mut report =
        json!({"kind":"actual-controller","performanceMeasured":false,"hardwareMotion":false});
    let result = exercise(&s, &root, &mut report).await;
    s.shutdown().await;
    report["shutdownSsh"] = s.devices.snapshot()["ssh"].clone();
    report["error"] = json!(result.as_ref().err().map(ToString::to_string));
    std::fs::write(
        root.join("real-connection-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    result
}
