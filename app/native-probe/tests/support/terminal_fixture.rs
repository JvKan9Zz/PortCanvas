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
#[path = "desktop_protocol.rs"]
pub mod desktop_protocol;

#[derive(Default)]
pub struct Counts {
    pub input: Mutex<BTreeMap<String, Vec<u8>>>,
    pub sizes: Mutex<Vec<(u32, u32)>>,
    pub closed: AtomicUsize,
    pub attached: AtomicUsize,
    pub active: AtomicUsize,
    pub terminal_eof: AtomicUsize,
    pub sessions: Mutex<BTreeMap<String, usize>>,
    pub completions: Mutex<Vec<serde_json::Value>>,
    pub desktop: Arc<desktop_protocol::Counts>,
}
struct Handler {
    counts: Arc<Counts>,
    channels: BTreeMap<ChannelId, Channel<server::Msg>>,
    terminals: BTreeMap<ChannelId, String>,
    controls: BTreeMap<ChannelId, String>,
    desktop_processes: std::collections::BTreeSet<ChannelId>,
    behavior: desktop_protocol::Behavior,
}
impl Drop for Handler {
    fn drop(&mut self) {
        self.counts
            .active
            .fetch_sub(self.terminals.len(), Ordering::SeqCst);
        self.counts.desktop.active_processes.fetch_sub(self.desktop_processes.len(), Ordering::SeqCst);
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
    async fn channel_open_direct_tcpip(
        &mut self, channel: Channel<server::Msg>, host: &str, port: u32,
        _: &str, _: u32, reply: server::ChannelOpenHandle, _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if host != "127.0.0.1" || port != 5901 { return Ok(()); }
        reply.accept().await;
        let counts = self.counts.desktop.clone();
        let behavior = self.behavior;
        tokio::spawn(async move { let _ = desktop_protocol::serve(channel.into_stream(), &counts, behavior).await; });
        Ok(())
    }
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
            if kind == "desktop" {
                self.counts.desktop.configs.lock().unwrap().push(value);
                self.desktop_processes.insert(id);
                self.counts.desktop.active_processes.fetch_add(1, Ordering::SeqCst);
                session.data(id, b"STUDIO_DESKTOP={\"port\":5901}\n".to_vec())?;
                return Ok(());
            }
            let reply = if kind == "environment" {
                format!(
                    "PORTCANVAS_ENV={}\n",
                    serde_json::json!({"ready":true,"feature":value["feature"]})
                )
            } else if kind == "completion" {
                self.counts.completions.lock().unwrap().push(value.clone());
                let prefix = value["prefix"].as_str().unwrap_or_default();
                if prefix == "slow" {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                let items = if value["directories"] == true {
                    serde_json::json!([{"value":"中文 目录/","label":"中文 目录/","kind":"directory"}])
                } else if prefix == "py" {
                    serde_json::json!([{"value":"python","label":"python","kind":"command"},{"value":"python3","label":"python3","kind":"command"}])
                } else if prefix == "中" {
                    serde_json::json!([{"value":"中文甲.txt","label":"中文甲.txt","kind":"file"},{"value":"中文乙.txt","label":"中文乙.txt","kind":"file"}])
                } else if prefix == "single" {
                    serde_json::json!([{"value":"single value's.txt","label":"single value's.txt","kind":"file"}])
                } else if prefix == "slow" {
                    serde_json::json!([{"value":"old-result","label":"old-result","kind":"file"}])
                } else if prefix == "many" {
                    serde_json::Value::Array((0..64).map(|n|serde_json::json!({"value":format!("many{n:02}"),"label":format!("many{n:02}"),"kind":"file"})).collect())
                } else {
                    serde_json::json!([])
                };
                serde_json::json!({"items":items,"truncated":prefix=="many","cwd":"/fixture"})
                    .to_string()
                    + "\n"
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
        if self.desktop_processes.remove(&id) {
            self.counts.desktop.active_processes.fetch_sub(1, Ordering::SeqCst);
            self.counts.desktop.process_eof.fetch_add(1, Ordering::SeqCst);
        }
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
        if self.desktop_processes.remove(&id) {
            self.counts.desktop.active_processes.fetch_sub(1, Ordering::SeqCst);
        }
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
        Self::with_behavior(desktop_protocol::Behavior::Normal).await
    }
    pub async fn with_behavior(behavior: desktop_protocol::Behavior) -> Result<Self> {
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
                    desktop_processes: Default::default(),
                    behavior,
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
        serde_json::json!({"attached":self.counts.attached.load(Ordering::SeqCst),"active":self.counts.active.load(Ordering::SeqCst),"terminalEof":self.counts.terminal_eof.load(Ordering::SeqCst),"closed":self.counts.closed.load(Ordering::SeqCst),"sizes":*self.counts.sizes.lock().unwrap(),"completionRequests":self.counts.completions.lock().unwrap().len(),
            "inputBytes":self.counts.input.lock().unwrap().iter().map(|(k,v)|(k.clone(),v.len())).collect::<BTreeMap<_,_>>(),"sessions":*self.counts.sessions.lock().unwrap(),"desktop":self.counts.desktop.report()})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.listener.abort();
    }
}
