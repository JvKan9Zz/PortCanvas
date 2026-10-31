//! SSH protocol fixture: interprets only the test protocol and never executes a command.
use anyhow::Result;
use base64::Engine;
use portcanvas_core::profiles::Profile;
use russh::{server, Channel, ChannelId};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::net::TcpListener;

#[derive(Default)]
pub struct Counts {
    pub input: Mutex<BTreeMap<String, Vec<u8>>>,
    pub sizes: Mutex<Vec<(u32, u32)>>,
    pub closed: AtomicUsize,
    pub attached: AtomicUsize,
    pub active: AtomicUsize,
    pub terminal_eof: AtomicUsize,
    pub sessions: Mutex<BTreeMap<String, usize>>,
}
struct Handler {
    counts: Arc<Counts>,
    channels: BTreeMap<ChannelId, Channel<server::Msg>>,
    terminals: BTreeMap<ChannelId, String>,
    controls: BTreeMap<ChannelId, String>,
}
impl Drop for Handler {
    fn drop(&mut self) {
        self.counts
            .active
            .fetch_sub(self.terminals.len(), Ordering::SeqCst);
    }
}
fn options(command: &str) -> Option<(String, serde_json::Value)> {
    command
        .split(|c: char| !c.is_ascii_alphanumeric() && !"+/=".contains(c))
        .find_map(|token| {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(token)
                .ok()?;
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            Some((
                value[0].as_str()?.into(),
                serde_json::from_str(value[1].as_str()?).ok()?,
            ))
        })
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        _: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if password == "synthetic-only" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn pty_request(
        &mut self,
        id: ChannelId,
        _: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.counts.sizes.lock().unwrap().push((cols, rows));
        session.channel_success(id)?;
        Ok(())
    }
    async fn window_change_request(
        &mut self,
        _: ChannelId,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.counts.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        command: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let command = String::from_utf8_lossy(command);
        session.channel_success(id)?;
        if let Some((kind, value)) = options(&command) {
            let reply = if kind == "environment" {
                format!(
                    "PORTCANVAS_ENV={}\n",
                    serde_json::json!({"ready":true,"feature":value["feature"]})
                )
            } else if kind == "logs" {
                let name = value["session"].as_str().unwrap().to_string();
                let mut sessions = self.counts.sessions.lock().unwrap();
                if value["action"] == "remove" {
                    sessions.remove(&name);
                } else if value["action"] == "reset" {
                    *sessions.entry(name.clone()).or_default() += 1;
                } else {
                    sessions.entry(name.clone()).or_insert(1);
                }
                serde_json::json!({"session":name,"pane":"%0","host":"owned-loopback","window":"synthetic","packet":""}).to_string()+"\n"
            } else {
                "{}\n".into()
            };
            session.data(id, reply.into_bytes())?;
            session.exit_status_request(id, 0)?;
            session.close(id)?;
        } else if command.contains("show-option -wqv") {
            session.data(id, b"ready;1-1;0;;;L2ZpeHR1cmU=;;;\n".to_vec())?;
            session.exit_status_request(id, 0)?;
            session.close(id)?;
        } else if command.contains(" -C attach-session -t ") {
            let name = command.split(" -t ").nth(1).unwrap().trim().to_string();
            self.controls.insert(id, name);
            session.data(id, b"%begin 1 1 0\n%end 1 1 0\n".to_vec())?;
        } else if command.contains(" attach-session -t =") {
            let name = command.split(" -t =").nth(1).unwrap().trim().to_string();
            self.terminals.insert(id, name.clone());
            let instance = self
                .counts
                .sessions
                .lock()
                .unwrap()
                .get(&name)
                .copied()
                .unwrap_or_default();
            self.counts.attached.fetch_add(1, Ordering::SeqCst);
            self.counts.active.fetch_add(1, Ordering::SeqCst);
            session.data(id,format!("\x1b[2J\x1b[HNative SSH PTY / instance {instance}\r\n中文宽字符 e\u{301} 🙂\r\n\x1b[32mANSI green\x1b[0m\r\n").into_bytes())?;
        } else {
            session.exit_status_request(id, 1)?;
            session.close(id)?;
        }
        Ok(())
    }
    async fn data(
        &mut self,
        id: ChannelId,
        bytes: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if let Some(name) = self.terminals.get(&id) {
            self.counts
                .input
                .lock()
                .unwrap()
                .entry(name.clone())
                .or_default()
                .extend_from_slice(bytes);
            if bytes == b"fixture-lines\r" {
                let output = (0..1400)
                    .map(|n| format!("line-{n:04}\r\n"))
                    .collect::<String>();
                session.data(id, output.into_bytes())?;
            } else if bytes == b"fixture-modes\r" {
                session.data(id, b"\x1b[?1h\x1b[?2004h\x1b[?1000h".to_vec())?;
            } else {
                session.data(id, bytes.to_vec())?;
            }
            // Mirror synthetic output into the shared log observer's tmux control stream.
            let encoded = bytes
                .iter()
                .map(|b| {
                    if !(32..127).contains(b) || *b == b'\\' {
                        format!("\\{:03o}", b)
                    } else {
                        (*b as char).to_string()
                    }
                })
                .collect::<String>();
            // Encode raw bytes, not Unicode code points, exactly like tmux control mode.
            for (control, target) in &self.controls {
                if target == name {
                    session.data(*control, format!("%output %0 {encoded}\n").into_bytes())?;
                }
            }
        }
        Ok(())
    }
    async fn channel_eof(
        &mut self,
        id: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if self.terminals.remove(&id).is_some() {
            self.counts.active.fetch_sub(1, Ordering::SeqCst);
            self.counts.terminal_eof.fetch_add(1, Ordering::SeqCst);
        }
        session.close(id)?;
        Ok(())
    }
    async fn channel_close(
        &mut self,
        id: ChannelId,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.channels.remove(&id);
        if self.terminals.remove(&id).is_some() {
            self.counts.active.fetch_sub(1, Ordering::SeqCst);
        }
        self.controls.remove(&id);
        self.counts.closed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
pub struct Fixture {
    pub profile: Profile,
    pub fingerprint: String,
    pub counts: Arc<Counts>,
    listener: tokio::task::JoinHandle<()>,
}
impl Fixture {
    pub async fn new() -> Result<Self> {
        let key =
            russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)?;
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
        let counts = Arc::new(Counts::default());
        let state = counts.clone();
        let listener = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let config = config.clone();
                let handler = Handler {
                    counts: state.clone(),
                    channels: BTreeMap::new(),
                    terminals: BTreeMap::new(),
                    controls: BTreeMap::new(),
                };
                tokio::spawn(async move {
                    if let Ok(session) = server::run_stream(config, socket, handler).await {
                        let _ = session.await;
                    }
                });
            }
        });
        let profile = serde_json::from_value(
            serde_json::json!({"id":"native-terminal-fixture","name":"本机终端验证","host":"127.0.0.1","port":port,"username":"fixture","display":":0","rememberPassword":true}),
        )?;
        Ok(Self {
            profile,
            fingerprint,
            counts,
            listener,
        })
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"attached":self.counts.attached.load(Ordering::SeqCst),"active":self.counts.active.load(Ordering::SeqCst),"terminalEof":self.counts.terminal_eof.load(Ordering::SeqCst),"closed":self.counts.closed.load(Ordering::SeqCst),"sizes":*self.counts.sizes.lock().unwrap(),
            "inputBytes":self.counts.input.lock().unwrap().iter().map(|(k,v)|(k.clone(),v.len())).collect::<BTreeMap<_,_>>(),"sessions":*self.counts.sessions.lock().unwrap()})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.listener.abort();
    }
}
