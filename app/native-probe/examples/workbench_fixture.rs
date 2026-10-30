//! Owned loopback server and isolated GUI data for native-workbench validation.
use anyhow::{Context, Result};
use portcanvas_core::profiles::{Profile, ProfileStore};
use russh::server;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::net::TcpListener;
struct Ssh(Arc<AtomicUsize>);
impl server::Handler for Ssh {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        _user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if password == "synthetic-only" {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::reject())
        }
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(
        std::env::var_os("WCS_TEST_ROOT").context("isolated WCS_TEST_ROOT required")?,
    );
    let data = root.join("workbench-gui");
    std::fs::create_dir_all(&data)?;
    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)?;
    let fingerprint = key
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    let config = Arc::new(server::Config {
        keys: vec![key],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let profile: Profile = serde_json::from_value(
        serde_json::json!({"id":"native-gui-fixture","name":"本机连接验证","host":"127.0.0.1","port":port,"username":"fixture","display":":0","rememberPassword":true}),
    )?;
    ProfileStore {
        directory: data.clone(),
    }
    .save(profile, "synthetic-only")?;
    let count = Arc::new(AtomicUsize::new(0));
    std::fs::write(
        root.join("fixture-ready.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"port":port,"fingerprint":fingerprint,"data":data}),
        )?,
    )?;
    let deadline = tokio::time::sleep(Duration::from_secs(600));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _=&mut deadline=>break,
            _=tokio::time::sleep(Duration::from_millis(100))=>{if root.join("fixture-stop").exists(){break;}},
            connection=listener.accept()=>{
                let(socket,_)=connection?;let config=config.clone();let count=count.clone();
                tokio::spawn(async move{if let Ok(session)=server::run_stream(config,socket,Ssh(count)).await{let _=session.await;}});
            }
        }
    }
    std::fs::write(
        root.join("fixture-result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"authentications":count.load(Ordering::SeqCst)}),
        )?,
    )?;
    Ok(())
}
