//! Opt-in real arena acceptance. Synthetic pose samples use a unique RAM path.
use anyhow::{ensure, Context, Result};
use portcanvas_core::{
    desktop::python,
    services::{Paths, Services},
};
use portcanvas_native_probe::camera_client;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
async fn until(mut predicate: impl FnMut() -> bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("arena observation timed out")
}
async fn exercise(s: &Arc<Services>, report: &mut Value) -> Result<()> {
    let profile = s
        .profiles
        .lock()
        .unwrap()
        .read()?
        .devices
        .into_iter()
        .next()
        .context("trusted isolated profile required")?;
    s.connect(
        profile,
        zeroize::Zeroizing::new(String::new()),
        Arc::new(|_, _| Box::pin(async { Ok(false) })),
    )
    .await?;
    let generation = s.devices.generation();
    let a = s.arena.metrics().await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let b = s.arena.metrics().await?;
    ensure!(b["clock"].as_f64() > a["clock"].as_f64());
    report["metrics"] = json!({"generation":generation,"cores":b["cores"],"memoryTotal":b["memoryTotal"],"twoMonotonicSamples":true});
    let path = format!(
        "/dev/shm/portcanvas-native-arena-{}.json",
        uuid::Uuid::new_v4()
    );
    std::env::set_var("PORTCANVAS_POSE_PATH", &path);
    let connection = s.devices.connection()?;
    let source="import json,time,os\np=json.loads(__import__('sys').argv[1]);path=p['path'];assert path.startswith('/dev/shm/portcanvas-native-arena-')\nif p['mode']=='remove':\n if os.path.exists(path):os.unlink(path)\nelse:\n v=dict(schema='portcanvas.pose/1',x=1.25,y=2.6,heading=90,timestamp=int(time.time()*1000))\n if p['mode']=='stale':v['timestamp']-=5000\n if p['mode']=='invalid':v['x']=True\n if p['mode']=='lost':v['valid']=False\n with open(path+'.tmp','x',encoding='utf-8') as f:json.dump(v,f)\n os.replace(path+'.tmp',path)\nprint('ok')";
    let pose = s
        .pose
        .start_for_generation(generation, CancellationToken::new())
        .await?;
    let result=async{
        until(||s.pose.snapshot()["phase"]=="unavailable").await?;
        for (mode,phase)in[("live","live"),("stale","stale"),("invalid","invalid"),("lost","unavailable")]{let command=python(source,json!(["fixture",json!({"path":path,"mode":mode}).to_string()]))?;connection.exec(&command,1000,Duration::from_secs(4)).await?;until(||s.pose.snapshot()["phase"]==phase).await?;s.pose.touch();}
        report["poseProtocol"]=json!({"missing":true,"liveSyntheticSample":true,"stale":true,"invalid":true,"trackingLost":true,"hardwarePositionMeasured":false});
        let devices=s.arena.cameras().await?;report["cameras"]=devices.clone();
        if std::env::var("PORTCANVAS_REAL_CAMERA").as_deref()==Ok("1"){
            let path=devices.as_array().into_iter().flatten().find(|v|v["accessible"]!=false).and_then(|v|v["path"].as_str()).context("accessible camera required")?.to_owned();
            let(tx,rx)=tokio::sync::watch::channel(None);let cancel=CancellationToken::new();let token=cancel.clone();let service=s.clone();
            let job=tokio::spawn(async move{camera_client::run(service,generation,path,20,tx,token).await});
            let seen=until(||rx.borrow().as_ref().is_some_and(|f|f.sequence>=3)).await;
            if let Some(frame)=rx.borrow().as_ref(){report["camera"]=json!({"realDevice":true,"width":frame.width,"height":frame.height,"sequence":frame.sequence,"transport":s.arena.status()["transport"],"streamUrlEmpty":s.arena.status()["streamUrl"]==""});}
            cancel.cancel();let stopped=tokio::time::timeout(Duration::from_secs(10),job).await;seen?;stopped???;
        }
        Ok::<_,anyhow::Error>(())
    }.await;
    s.pose.stop(pose["id"].as_u64()).await;
    let command = python(
        source,
        json!(["fixture", json!({"path":path,"mode":"remove"}).to_string()]),
    )?;
    connection
        .exec(&command, 1000, Duration::from_secs(4))
        .await?;
    report["testPoseRemoved"] = json!(true);
    result
}
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").context("WCS_TEST_ROOT required")?);
    ensure!(root.is_absolute() && root.join("data/devices.json").is_file());
    let s = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.clone(),
            local_files: root.clone(),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("native-arena-acceptance".into()),
    )?;
    let mut report = json!({"kind":"real-controller","poseSamples":"explicit synthetic protocol fixtures","hardwareMotion":false,"performanceBenchmark":false});
    let result = exercise(&s, &mut report).await;
    s.shutdown().await;
    report["error"] = json!(result.as_ref().err().map(|e| format!("{e:#}")));
    report["shutdownSsh"] = s.devices.snapshot()["ssh"].clone();
    std::fs::write(
        root.join("real-arena-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    result
}
