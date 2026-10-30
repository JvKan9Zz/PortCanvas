//! Owned loopback SSH; no device or system command execution.
use anyhow::Result;
use portcanvas_core::{
    application::HostConfirmation,
    profiles::Profile,
    services::{Paths, Services},
};
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
use zeroize::Zeroizing;

struct Ssh {
    authenticated: Arc<AtomicUsize>,
}
impl server::Handler for Ssh {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        _user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if password == "synthetic-only" {
            self.authenticated.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::reject())
        }
    }
}
struct Fixture {
    services: Arc<Services>,
    profile: Profile,
    root: PathBuf,
    server: tokio::task::JoinHandle<()>,
    authenticated: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new() -> Result<Self> {
        let root =
            PathBuf::from(std::env::var_os("WCS_TEST_ROOT").expect("isolated WCS_TEST_ROOT"))
                .join(format!("application-{}", uuid::Uuid::new_v4()));
        let services = Services::new(
            Paths {
                data: root.join("data"),
                documents: root.join("documents"),
                local_files: root.clone(),
                portable: true,
            },
            Arc::new(|_, _| {}),
            None,
        )?;
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)?;
        let config = Arc::new(server::Config {
            keys: vec![key],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let authenticated = Arc::new(AtomicUsize::new(0));
        let count = authenticated.clone();
        let server = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let config = config.clone();
                let authenticated = count.clone();
                tokio::spawn(async move {
                    if let Ok(session) =
                        server::run_stream(config, socket, Ssh { authenticated }).await
                    {
                        let _ = session.await;
                    }
                });
            }
        });
        let profile: Profile = serde_json::from_value(
            serde_json::json!({"id":"native-loopback","name":"本机测试","host":"127.0.0.1","port":port,"username":"fixture","display":":0","rememberPassword":true}),
        )?;
        let profile = services.save_profile(profile, "synthetic-only")?;
        Ok(Self {
            services,
            profile,
            root,
            server,
            authenticated,
        })
    }
    async fn close(self) -> Result<()> {
        self.services.shutdown().await;
        self.server.abort();
        drop(self.services);
        std::fs::remove_dir_all(self.root)?;
        Ok(())
    }
}
fn decision(yes: bool) -> HostConfirmation {
    Arc::new(move |_, _| Box::pin(async move { Ok(yes) }))
}
fn saved() -> Zeroizing<String> {
    Zeroizing::new(String::new())
}
fn retained_fingerprint() -> String {
    format!("SHA256:{}", "A".repeat(43))
}

#[tokio::test]
async fn explicit_identity_acceptance_and_saved_credentials_work_for_both_callers() -> Result<()> {
    let f = Fixture::new().await?;
    let state = f
        .services
        .connect(f.profile.clone(), saved(), decision(true))
        .await?;
    assert_eq!(state["ssh"], "connected");
    assert_eq!(f.authenticated.load(Ordering::SeqCst), 1);
    assert!(f.services.save_profile(f.profile.clone(), "").is_err());
    let original = f.services.profiles.lock().unwrap().read()?.host_keys;
    assert_eq!(original.len(), 1);
    f.services.disconnect().await;
    let should_not_prompt: HostConfirmation =
        Arc::new(|_, _| Box::pin(async { panic!("known key must not prompt again") }));
    f.services
        .connect(f.profile.clone(), saved(), should_not_prompt)
        .await?;
    assert_eq!(f.authenticated.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.services.profiles.lock().unwrap().read()?.host_keys,
        original
    );
    f.close().await
}
#[tokio::test]
async fn denied_and_changed_identities_never_authenticate_or_rewrite_trust() -> Result<()> {
    let f = Fixture::new().await?;
    assert!(f
        .services
        .connect(f.profile.clone(), saved(), decision(false))
        .await
        .is_err());
    assert_eq!(f.authenticated.load(Ordering::SeqCst), 0);
    assert!(f
        .services
        .profiles
        .lock()
        .unwrap()
        .read()?
        .host_keys
        .is_empty());
    f.services
        .profiles
        .lock()
        .unwrap()
        .trust(&f.profile.address(), &retained_fingerprint())?;
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let prompt: HostConfirmation = Arc::new(move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    });
    assert!(f
        .services
        .connect(f.profile.clone(), saved(), prompt)
        .await
        .unwrap_err()
        .to_string()
        .contains("主机身份已变化"));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert_eq!(f.authenticated.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.services.profiles.lock().unwrap().read()?.host_keys[&f.profile.address()],
        retained_fingerprint()
    );
    f.close().await
}
#[tokio::test]
async fn cancellation_during_identity_prompt_cannot_save_late_acceptance() -> Result<()> {
    let f = Fixture::new().await?;
    let (ready, started) = tokio::sync::oneshot::channel();
    let ready = Arc::new(std::sync::Mutex::new(Some(ready)));
    let prompt: HostConfirmation = Arc::new(move |_, cancel| {
        let ready = ready.clone();
        Box::pin(async move {
            if let Some(ready) = ready.lock().unwrap().take() {
                let _ = ready.send(());
            }
            cancel.cancelled().await;
            Ok(true)
        })
    });
    let services = f.services.clone();
    let profile = f.profile.clone();
    let connect = tokio::spawn(async move { services.connect(profile, saved(), prompt).await });
    tokio::time::timeout(Duration::from_secs(5), started).await??;
    f.services.disconnect().await;
    assert!(tokio::time::timeout(Duration::from_secs(5), connect)
        .await??
        .is_err());
    assert!(f
        .services
        .profiles
        .lock()
        .unwrap()
        .read()?
        .host_keys
        .is_empty());
    assert_eq!(f.authenticated.load(Ordering::SeqCst), 0);
    f.close().await
}
#[tokio::test]
async fn removal_keeps_trust_until_the_last_matching_address_is_removed() -> Result<()> {
    let f = Fixture::new().await?;
    let mut other = f.profile.clone();
    other.id = "other-profile".into();
    other.remember_password = false;
    f.services.save_profile(other, "")?;
    f.services
        .profiles
        .lock()
        .unwrap()
        .trust(&f.profile.address(), &retained_fingerprint())?;
    f.services.remove_profile(&f.profile.id).await?;
    assert_eq!(
        f.services.profiles.lock().unwrap().read()?.host_keys.len(),
        1
    );
    f.services.remove_profile("other-profile").await?;
    assert!(f
        .services
        .profiles
        .lock()
        .unwrap()
        .read()?
        .host_keys
        .is_empty());
    f.close().await
}
