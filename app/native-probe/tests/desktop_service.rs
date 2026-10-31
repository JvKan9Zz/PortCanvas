#[path = "support/terminal_fixture.rs"]
mod support;
use anyhow::Result;
use portcanvas_core::{
    desktop_settings::DesktopMode,
    services::{Paths, Services},
};
use portcanvas_native_probe::{
    desktop_client::{self, Channels, Command, Input, Motion, State},
    rfb::Frame,
};
use std::{
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

async fn until(label: &str, mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out: {label}"));
}
async fn connect(fixture: &support::Fixture) -> Result<(Arc<Services>, PathBuf)> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap())
        .join(format!("desktop-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root)?;
    let services = Services::new(
        Paths {
            data: root.join("data"),
            documents: root.clone(),
            local_files: root.clone(),
            portable: true,
        },
        Arc::new(|_, _| {}),
        Some("native-desktop-tests".into()),
    )?;
    let fingerprint = fixture.fingerprint.clone();
    services
        .connect(
            fixture.profile.clone(),
            zeroize::Zeroizing::new("synthetic-only".into()),
            Arc::new(move |identity, _| {
                let matches = identity.fingerprint == fingerprint;
                Box::pin(async move { Ok(matches) })
            }),
        )
        .await?;
    Ok((services, root))
}
struct Client {
    input: mpsc::Sender<Input>,
    _motion: watch::Sender<Motion>,
    frames: watch::Receiver<Option<Arc<Frame>>>,
    clipboard: watch::Receiver<Option<String>>,
    states: Arc<Mutex<Vec<State>>>,
    cancel: CancellationToken,
    job: tokio::task::JoinHandle<Result<()>>,
}
impl Client {
    fn new(services: Arc<Services>, mode: DesktopMode) -> Self {
        let (input, inputs) = mpsc::channel(64);
        let (motion, motions) = watch::channel(Motion::default());
        let (frames, frame_rx) = watch::channel(None);
        let (clipboard, clip_rx) = watch::channel(None);
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = states.clone();
        let cancel = CancellationToken::new();
        let generation = services.devices.generation();
        let token = cancel.clone();
        let job = tokio::spawn(async move {
            desktop_client::run(
                services,
                generation,
                mode,
                Channels {
                    inputs,
                    motion: motions,
                    frames,
                    clipboard,
                    state: Arc::new(move |s| sink.lock().unwrap().push(s)),
                },
                token,
            )
            .await
        });
        Self {
            input,
            _motion: motion,
            frames: frame_rx,
            clipboard: clip_rx,
            states,
            cancel,
            job,
        }
    }
    fn frame(&self) -> Arc<Frame> {
        self.frames.borrow().clone().unwrap()
    }
    async fn command(&self, view: u64, command: Command) -> Result<()> {
        self.input.send(Input { view, command }).await?;
        Ok(())
    }
    async fn stop(self) -> Result<()> {
        self.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(8), self.job).await???;
        Ok(())
    }
}
async fn released(f: &support::Fixture) {
    until("owned desktop channels released", || {
        f.counts.desktop.active_processes.load(Ordering::SeqCst) == 0
            && f.counts.desktop.active_streams.load(Ordering::SeqCst) == 0
    })
    .await;
    assert!(f.counts.desktop.process_eof.load(Ordering::SeqCst) > 0);
}
#[tokio::test]
async fn native_desktop_over_shared_ssh_preserves_quality_pixels_input_resize_and_cleanup(
) -> Result<()> {
    let fixture = support::Fixture::new().await?;
    let (services, root) = connect(&fixture).await?;
    assert!(services
        .desktop
        .start_native(
            DesktopMode::Smooth,
            services.devices.generation() + 1,
            CancellationToken::new()
        )
        .await
        .is_err());
    assert!(fixture.counts.desktop.configs.lock().unwrap().is_empty());
    for (index, mode, quality) in [
        (0, DesktopMode::Smooth, 5),
        (1, DesktopMode::Clear, 8),
        (2, DesktopMode::Economy, 4),
    ] {
        let initial_requests = fixture.counts.desktop.requests.load(Ordering::SeqCst);
        let initial_full = fixture.counts.desktop.full_requests.load(Ordering::SeqCst);
        let client = Client::new(services.clone(), mode);
        until("complete first framebuffer", || {
            client.frames.borrow().is_some()
        })
        .await;
        let frame = client.frame();
        let view = frame.view;
        assert_eq!((frame.width, frame.height), (320, 180));
        assert!(frame.pixels.chunks_exact(4).all(|p| p[3] == 255));
        let cursor = frame.cursor.as_ref().unwrap();
        assert_eq!(
            (cursor.width, cursor.height, cursor.hot_x, cursor.hot_y),
            (8, 8, 1, 1)
        );
        assert!(cursor.pixels.chunks_exact(4).any(|p| p[3] == 0));
        until("same-size resize announcement consumed", || {
            fixture.counts.desktop.requests.load(Ordering::SeqCst) >= initial_requests + 3
        })
        .await;
        let requests = fixture.counts.desktop.requests.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            fixture.counts.desktop.requests.load(Ordering::SeqCst),
            requests,
            "static desktop must have one outstanding incremental request"
        );
        assert_eq!(
            fixture.counts.desktop.full_requests.load(Ordering::SeqCst),
            initial_full + 1,
            "same-size announcement must retain valid pixels"
        );
        {
            let enc = fixture.counts.desktop.encodings.lock().unwrap();
            assert!(enc[index].contains(&(-32 + quality)));
            assert!(enc[index].contains(&-255));
        }
        if mode == DesktopMode::Economy {
            let configs = fixture.counts.desktop.configs.lock().unwrap();
            assert_eq!(configs[index]["tuning"]["maxWidth"], 960);
            assert_eq!(configs[index]["tuning"]["maxHeight"], 540);
        }
        if index == 0 {
            client.command(view, Command::Text("中🙂".into())).await?;
            client.command(view, Command::Key(0xffe3, true)).await?;
            let point = Motion {
                sequence: 1,
                view,
                x: 100,
                y: 60,
            };
            client.command(view, Command::Pointer(point, 1)).await?;
            client
                .command(
                    view,
                    Command::Wheel(
                        Motion {
                            sequence: 2,
                            ..point
                        },
                        8,
                        2,
                    ),
                )
                .await?;
            client
                .command(view, Command::Clipboard("café".into()))
                .await?;
            until("Latin-1 clipboard echo", || {
                client.clipboard.borrow().as_deref() == Some("café")
            })
            .await;
            assert_eq!(
                fixture.counts.desktop.clipboard.lock().unwrap()[0],
                b"caf\xe9"
            );
            {
                let keys = fixture.counts.desktop.keys.lock().unwrap();
                assert!(keys.contains(&(0x01004e2d, true)));
                assert!(keys.contains(&(0x0101f642, false)));
            }
            client.command(view, Command::Release(3)).await?;
            until("focus release", || {
                fixture.counts.desktop.held.lock().unwrap().is_empty()
                    && fixture
                        .counts
                        .desktop
                        .pointers
                        .lock()
                        .unwrap()
                        .last()
                        .is_some_and(|p| p.2 == 0)
            })
            .await;
            client.command(view, Command::Key(0xffbf, true)).await?; // F2 changes only the synthetic framebuffer.
            until("resized first frame", || {
                client
                    .frames
                    .borrow()
                    .as_ref()
                    .is_some_and(|f| f.view > view && f.width == 240)
            })
            .await;
            let resized = client.frame();
            assert_eq!((resized.width, resized.height), (240, 240));
            assert!(client
                .states
                .lock()
                .unwrap()
                .iter()
                .any(|s| s.view == resized.view && !s.ready));
            client.command(view, Command::Key(0x5a, true)).await?; // stale coordinates/keys from the old image must be ignored.
            client
                .command(resized.view, Command::Key(0x59, true))
                .await?;
            client
                .command(
                    resized.view,
                    Command::Pointer(
                        Motion {
                            sequence: 4,
                            view: resized.view,
                            x: 30,
                            y: 40,
                        },
                        4,
                    ),
                )
                .await?;
            until("new-view input", || {
                fixture.counts.desktop.held.lock().unwrap().contains(&0x59)
            })
            .await;
            assert!(!fixture
                .counts
                .desktop
                .keys
                .lock()
                .unwrap()
                .contains(&(0x5a, true)));
        }
        client.stop().await?;
        released(&fixture).await;
        assert!(fixture.counts.desktop.held.lock().unwrap().is_empty());
        assert_eq!(
            fixture
                .counts
                .desktop
                .pointers
                .lock()
                .unwrap()
                .last()
                .map(|p| p.2)
                .unwrap_or(0),
            0
        );
        assert_eq!(services.devices.snapshot()["ssh"], "connected");
    }
    services.shutdown().await;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&fixture.report())?,
    )?;
    Ok(())
}
#[tokio::test]
async fn incomplete_first_frame_never_allows_input_and_cancellation_releases_tunnel() -> Result<()>
{
    for behavior in [
        support::desktop_protocol::Behavior::NoFirstFrame,
        support::desktop_protocol::Behavior::PartialFirstFrame,
    ] {
        let fixture = support::Fixture::with_behavior(behavior).await?;
        let (services, root) = connect(&fixture).await?;
        let client = Client::new(services.clone(), DesktopMode::Smooth);
        until("first frame request", || {
            fixture.counts.desktop.requests.load(Ordering::SeqCst) > 0
        })
        .await;
        client.command(1, Command::Key(65, true)).await?;
        client
            .command(
                1,
                Command::Pointer(
                    Motion {
                        sequence: 1,
                        view: 1,
                        x: 1,
                        y: 1,
                    },
                    1,
                ),
            )
            .await?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(client.frames.borrow().is_none());
        assert!(!client.states.lock().unwrap().iter().any(|s| s.ready));
        assert!(fixture.counts.desktop.keys.lock().unwrap().is_empty());
        assert!(fixture.counts.desktop.pointers.lock().unwrap().is_empty());
        client.stop().await?;
        released(&fixture).await;
        assert_eq!(services.devices.snapshot()["ssh"], "connected");
        services.shutdown().await;
        std::fs::write(
            root.join("result.json"),
            serde_json::to_vec_pretty(&fixture.report())?,
        )?;
    }
    Ok(())
}
#[tokio::test]
async fn additional_vnc_authentication_is_rejected_without_leaking_owned_process() -> Result<()> {
    let fixture =
        support::Fixture::with_behavior(support::desktop_protocol::Behavior::ExtraAuthentication)
            .await?;
    let (services, root) = connect(&fixture).await?;
    let client = Client::new(services.clone(), DesktopMode::Smooth);
    let result = tokio::time::timeout(Duration::from_secs(10), client.job).await??;
    assert!(result.is_err());
    released(&fixture).await;
    assert_eq!(services.devices.snapshot()["ssh"], "connected");
    assert_eq!(services.devices.snapshot()["desktop"], "error");
    services.shutdown().await;
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&fixture.report())?,
    )?;
    Ok(())
}
