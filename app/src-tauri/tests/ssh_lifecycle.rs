//! Real SSH packets over loopback; the mock never executes a submitted command.
use russh::{server, Channel, ChannelId};
use std::{
    collections::HashMap,
    sync::{atomic::{AtomicUsize, Ordering}, Arc},
    time::Duration,
};
use studio_core::{
    desktop::DesktopService, desktop_settings::DesktopMode, profiles::Profile,
    session::DeviceManager, ssh::{HostCheck, SshConnection},
};
use tokio::{net::TcpListener, sync::Notify};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[derive(Default)]
struct Counts {
    exec: AtomicUsize,
    eof: AtomicUsize,
    close: AtomicUsize,
    subsystem: AtomicUsize,
    changed: Notify,
}
impl Counts {
    async fn wait_for_cleanup(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.eof.load(Ordering::SeqCst) + self.close.load(Ordering::SeqCst) == 0 {
                self.changed.notified().await;
            }
        }).await.expect("owned SSH channel did not receive EOF or Close");
    }
}
struct MockSsh {
    counts: Arc<Counts>,
    channels: HashMap<ChannelId, Channel<server::Msg>>,
    environment_reply: bool,
}
impl server::Handler for MockSsh {
    type Error = russh::Error;
    async fn auth_password(&mut self, _: &str, password: &str) -> Result<server::Auth, Self::Error> {
        Ok(if password == "synthetic-local-only" { server::Auth::Accept } else { server::Auth::reject() })
    }
    async fn channel_open_session(&mut self, channel: Channel<server::Msg>, reply: server::ChannelOpenHandle, _: &mut server::Session) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn exec_request(&mut self, id: ChannelId, _: &[u8], session: &mut server::Session) -> Result<(), Self::Error> {
        self.counts.exec.fetch_add(1, Ordering::SeqCst);
        self.counts.changed.notify_one();
        session.channel_success(id)?;
        if self.environment_reply {
            session.data(id, b"PORTCANVAS_ENV={\"ready\":true,\"feature\":\"terminal\",\"binDir\":\"/tmp/fixture's/bin\",\"tmux\":\"/tmp/fixture's/bin/tmux\",\"sftp\":\"/fixture/system/sftp-server\"}\n".to_vec())?;
            session.exit_status_request(id, 0)?;
            session.close(id)?;
        }
        Ok(())
    }
    async fn subsystem_request(&mut self, id: ChannelId, name: &str, session: &mut server::Session) -> Result<(), Self::Error> {
        assert_eq!(name, "sftp");
        self.counts.subsystem.fetch_add(1, Ordering::SeqCst);
        self.counts.changed.notify_one();
        session.channel_success(id)?;
        Ok(())
    }
    async fn channel_eof(&mut self, id: ChannelId, session: &mut server::Session) -> Result<(), Self::Error> {
        self.counts.eof.fetch_add(1, Ordering::SeqCst);
        self.counts.changed.notify_one();
        session.exit_status_request(id, 0)?;
        session.close(id)?;
        Ok(())
    }
    async fn channel_close(&mut self, id: ChannelId, _: &mut server::Session) -> Result<(), Self::Error> {
        self.counts.close.fetch_add(1, Ordering::SeqCst);
        self.counts.changed.notify_one();
        self.channels.remove(&id);
        Ok(())
    }
}
struct Fixture {
    profile: Profile,
    counts: Arc<Counts>,
    listener: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_environment_reply(false).await
    }
    async fn with_environment_reply(environment_reply: bool) -> Self {
        // Fresh in-memory key: no committed or on-disk private-key fixture.
        let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519).unwrap();
        let config = Arc::new(server::Config {
            keys: vec![key], auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO), ..Default::default()
        });
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let counts = Arc::new(Counts::default());
        let state = counts.clone();
        let listener = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let handler = MockSsh { counts: state.clone(), channels: HashMap::new(), environment_reply };
                let config = config.clone();
                tokio::spawn(async move {
                    if let Ok(session) = server::run_stream(config, stream, handler).await { let _ = session.await; }
                });
            }
        });
        let profile = serde_json::from_value(serde_json::json!({
            "id":"ssh-lifecycle-test", "name":"Loopback fixture", "host":"127.0.0.1",
            "port":port, "username":"fixture", "display":":0"
        })).unwrap();
        Self { profile, counts, listener }
    }
    async fn connect(&self) -> SshConnection {
        SshConnection::connect(self.profile.clone(), secret(), check(true), CancellationToken::new()).await.unwrap()
    }
}
impl Drop for Fixture { fn drop(&mut self) { self.listener.abort(); } }
fn check(accept: bool) -> HostCheck { Arc::new(move |_| Box::pin(async move { Ok(accept) })) }
fn secret() -> Zeroizing<String> { Zeroizing::new("synthetic-local-only".into()) }

#[tokio::test]
async fn host_denial_wrong_password_and_cancellation_are_rejected() {
    let fixture = Fixture::new().await;
    assert!(SshConnection::connect(fixture.profile.clone(), secret(), check(false), CancellationToken::new()).await.is_err());
    assert!(SshConnection::connect(fixture.profile.clone(), Zeroizing::new("wrong-synthetic".into()), check(true), CancellationToken::new()).await.is_err());
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(SshConnection::connect(fixture.profile.clone(), secret(), check(true), cancel).await.is_err());
}

#[tokio::test]
async fn exec_timeout_closes_its_channel_and_preserves_the_connection() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;
    let result = connection.exec("not-executed", 1024, Duration::from_millis(100)).await;
    assert!(result.unwrap_err().to_string().contains("远端命令超时"));
    fixture.counts.wait_for_cleanup().await;
    assert!(!connection.handle.is_closed());
    connection.disconnect().await;
}

#[tokio::test]
async fn desktop_start_timeout_closes_its_owned_process_channel() {
    let fixture = Fixture::new().await;
    let devices = DeviceManager::new(Arc::new(|_, _| {}));
    devices.connect(fixture.profile.clone(), secret(), |_| check(true)).await.unwrap();
    let desktop = DesktopService::new(devices.clone());
    let result = tokio::time::timeout(Duration::from_secs(30), desktop.start("http://tauri.localhost".into(), DesktopMode::Smooth)).await.unwrap();
    assert!(result.unwrap_err().to_string().contains("桌面启动超时"));
    fixture.counts.wait_for_cleanup().await;
    assert_eq!(fixture.counts.exec.load(Ordering::SeqCst), 1);
    assert!(devices.connection().is_ok());
    devices.disconnect().await;
}

#[tokio::test]
async fn explicit_close_is_observable_by_the_fixture() {
    let fixture = Fixture::new().await;
    let connection = fixture.connect().await;
    let channel = connection.channel().await.unwrap();
    channel.exec(true, "not-executed").await.unwrap();
    channel.eof().await.unwrap();
    channel.close().await.unwrap();
    fixture.counts.wait_for_cleanup().await;
    connection.disconnect().await;
}

#[tokio::test]
async fn environment_preparation_is_cached_and_quotes_private_paths() {
    let fixture = Fixture::with_environment_reply(true).await;
    let devices = DeviceManager::new(Arc::new(|_, _| {})).with_environment();
    devices.connect(fixture.profile.clone(), secret(), |_| check(true)).await.unwrap();
    let connection = devices.connection().unwrap();
    for _ in 0..2 {
        connection.prepare(studio_core::remote_env::Feature::Terminal, &connection.cancel).await.unwrap();
    }
    assert_eq!(fixture.counts.exec.load(Ordering::SeqCst), 1);
    assert!(connection.remote_command("printf test").contains("'/tmp/fixture'\\''s/bin'"));
    assert!(connection.remote_command("printf test").ends_with("printf test"));
    let channel = connection.sftp_channel().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.counts.subsystem.load(Ordering::SeqCst) == 0 { fixture.counts.changed.notified().await; }
    }).await.expect("terminal preparation must not replace the existing SFTP subsystem");
    channel.close().await.unwrap();
    assert_eq!(fixture.counts.exec.load(Ordering::SeqCst), 1);
    devices.disconnect().await;
}

#[tokio::test]
async fn canceling_environment_preparation_closes_only_the_owned_channel() {
    let fixture = Fixture::new().await;
    let devices = DeviceManager::new(Arc::new(|_, _| {})).with_environment();
    devices.connect(fixture.profile.clone(), secret(), |_| check(true)).await.unwrap();
    let connection = devices.connection().unwrap();
    let cancel = CancellationToken::new();
    let preparing = {
        let connection = connection.clone(); let cancel = cancel.clone();
        tokio::spawn(async move { connection.prepare(studio_core::remote_env::Feature::Desktop, &cancel).await })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        while fixture.counts.exec.load(Ordering::SeqCst) == 0 { fixture.counts.changed.notified().await; }
    }).await.unwrap();
    cancel.cancel();
    assert!(preparing.await.unwrap().unwrap_err().to_string().contains("取消"));
    fixture.counts.wait_for_cleanup().await;
    assert!(devices.connection().is_ok());
    devices.disconnect().await;
}
