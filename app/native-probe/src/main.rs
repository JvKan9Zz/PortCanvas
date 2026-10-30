#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod ui;
use anyhow::Result;
use portcanvas_native_probe::{fixture, media, rfb, terminal::Terminal};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};
use ui::{ProbeWindow, TerminalCell};

fn refresh(ui: &ProbeWindow, terminal: &Terminal) {
    let cells = terminal
        .cells()
        .into_iter()
        .filter(|c| c.text != " " || c.background != [24, 28, 36])
        .map(|c| TerminalCell {
            column: c.column as i32,
            row: c.row as i32,
            text: c.text.into(),
            ink: slint::Color::from_rgb_u8(c.foreground[0], c.foreground[1], c.foreground[2]),
            paper: slint::Color::from_rgb_u8(c.background[0], c.background[1], c.background[2]),
            wide: c.wide,
            bold: c.bold,
            underline: c.underline,
        })
        .collect::<Vec<_>>();
    ui.set_cells(ModelRc::new(VecModel::from(cells)));
    let (column, row) = terminal.cursor();
    ui.set_cursor_column(column as i32);
    ui.set_cursor_row(row as i32);
}
fn picture(width: u32, height: u32, rgba: &[u8]) -> slint::Image {
    slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba, width, height),
    )
}
fn sample(terminal: &mut Terminal) {
    terminal.feed(b"\x1b[?1049l\x1b[2J\x1b[H");
    terminal.feed("PortCanvas native VT\r\n中文宽字符 / e\u{301} / 🙂\r\n\x1b[32mANSI green\x1b[0m  \x1b[38;2;100;170;240mTrue color\x1b[0m\r\n".as_bytes());
    for n in 0..12 {
        terminal.feed(format!("line {n:02}  native terminal buffer\r\n").as_bytes());
    }
}
fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--core-smoke") {
        let root = PathBuf::from(
            std::env::var_os("WCS_TEST_ROOT")
                .ok_or_else(|| anyhow::anyhow!("Isolated WCS_TEST_ROOT required"))?,
        )
        .join(format!("native-core-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let services = portcanvas_core::services::Services::new(
            portcanvas_core::services::Paths {
                data: root.join("data"),
                documents: root.join("documents"),
                local_files: root.clone(),
                portable: true,
            },
            Arc::new(|_, _| {}),
            Some("native-core-smoke".into()),
        )?;
        let result = serde_json::json!({"core":"portcanvas-core","ssh":services.devices.snapshot()["ssh"],
            "profiles":services.profiles.lock().unwrap().read()?.devices.len(),"language":services.preferences.lock().unwrap().info()?["language"]});
        runtime.block_on(services.shutdown());
        drop(services);
        std::fs::remove_dir_all(root)?;
        println!("{result}");
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--media-fixture") {
        return media::write_fixture(&PathBuf::from(
            std::env::args_os().nth(2).expect("fixture path"),
        ));
    }
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg".into())
        .select()?;
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );
    let ui = ProbeWindow::new()?;
    let terminal = Rc::new(RefCell::new(Terminal::new(100, 22)));
    sample(&mut terminal.borrow_mut());
    refresh(&ui, &terminal.borrow());
    let (_desktop_tx, desktop_rx) = watch::channel(rfb::Frame::default());
    let (video_tx, video_rx) = watch::channel(None::<media::VideoFrame>);
    let input_tx = Rc::new(RefCell::new(None::<mpsc::Sender<vnc::X11Event>>));
    let desktop_stop = Rc::new(RefCell::new(None::<watch::Sender<bool>>));
    let video_stop = Rc::new(RefCell::new(None::<Arc<AtomicBool>>));
    let video_count = Arc::new(AtomicUsize::new(0));
    let desktop_count = Rc::new(RefCell::new(0usize));
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let lifecycle = Rc::new(RefCell::new(Vec::<serde_json::Value>::new()));
    let video_jobs = Rc::new(RefCell::new(Vec::new()));
    let timer = Rc::new(slint::Timer::default());
    let receivers = Rc::new(RefCell::new((desktop_rx, video_rx)));
    let start_render = {
        let timer = timer.clone();
        let receivers = receivers.clone();
        let weak = ui.as_weak();
        let count = desktop_count.clone();
        move || {
            let receivers = receivers.clone();
            let weak = weak.clone();
            let count = count.clone();
            timer.start(
                slint::TimerMode::Repeated,
                Duration::from_millis(33),
                move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let mut rx = receivers.borrow_mut();
                    if rx.0.has_changed().unwrap_or(false) {
                        let frame = rx.0.borrow_and_update();
                        if frame.width > 0 {
                            ui.set_desktop(picture(
                                frame.width.into(),
                                frame.height.into(),
                                &frame.pixels,
                            ));
                            *count.borrow_mut() += 1;
                        }
                    }
                    if rx.1.has_changed().unwrap_or(false) {
                        if let Some(frame) = rx.1.borrow_and_update().as_ref() {
                            ui.set_video(picture(frame.width, frame.height, &frame.rgba));
                        }
                    }
                },
            );
        }
    };
    let stop = {
        let desktop_stop = desktop_stop.clone();
        let input_tx = input_tx.clone();
        let video_stop = video_stop.clone();
        let timer = timer.clone();
        move || {
            if let Some(sender) = desktop_stop.borrow_mut().take() {
                sender.send_replace(true);
            }
            input_tx.borrow_mut().take();
            if let Some(flag) = video_stop.borrow_mut().take() {
                flag.store(true, Ordering::SeqCst);
            }
            timer.stop();
        }
    };
    ui.on_stop(stop.clone());
    ui.on_page_changed({
        let stop = stop.clone();
        move |_| stop()
    });
    ui.window().on_winit_window_event({let stop=stop.clone();let count=video_count.clone();let lifecycle=lifecycle.clone();move |window,event|{
        if matches!(event,winit::event::WindowEvent::Occluded(true)) || (matches!(event,winit::event::WindowEvent::Resized(_)) && window.with_winit_window(|w|w.is_minimized()==Some(true)).unwrap_or(false)){
            lifecycle.borrow_mut().push(serde_json::json!({"event":"native-hidden","videoFrames":count.load(Ordering::Relaxed)}));stop();
        }
        EventResult::Propagate
    }});
    ui.on_sample({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move || {
            sample(&mut terminal.borrow_mut());
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_alternate({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        let alternate = Rc::new(RefCell::new(false));
        move || {
            let mut flag = alternate.borrow_mut();
            *flag = !*flag;
            terminal.borrow_mut().feed(if *flag {
                b"\x1b[?1049h\x1b[2J\x1b[HNative alternate screen\r\nFull-screen VT buffer"
            } else {
                b"\x1b[?1049l"
            });
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_submit({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move |text| {
            terminal.borrow_mut().feed(format!("{text}\r\n").as_bytes());
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
                ui.set_status("本机回显".into());
            }
        }
    });
    ui.on_scroll({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move |lines| {
            terminal.borrow_mut().scroll(lines);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_resize_grid({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move |columns, rows| {
            terminal
                .borrow_mut()
                .resize(columns.max(2) as usize, rows.max(1) as usize);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_select({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move |x, y, x2, y2| {
            terminal
                .borrow_mut()
                .select((x as usize, y as usize), (x2 as usize, y2 as usize));
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_copy({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move || {
            if let Some(text) = terminal.borrow().selected_text() {
                let result =
                    arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text));
                if let Some(ui) = weak.upgrade() {
                    ui.set_status(match result {
                        Ok(()) => "已复制选区".into(),
                        Err(e) => e.to_string().into(),
                    });
                }
            }
        }
    });
    ui.on_key({
        let weak = ui.as_weak();
        let terminal = terminal.clone();
        move |key, control, _shift| {
            let bytes = if control {
                key.chars()
                    .next()
                    .map(|c| vec![(c as u8) & 31])
                    .unwrap_or_default()
            } else {
                match key.as_str() {
                    "\n" => b"\r\n".to_vec(),
                    "\u{8}" => b"\x08 \x08".to_vec(),
                    text if !text.chars().any(char::is_control)
                        && !text.chars().any(|c| ('\u{f700}'..='\u{f8ff}').contains(&c)) =>
                    {
                        text.as_bytes().to_vec()
                    }
                    _ => Vec::new(),
                }
            };
            terminal.borrow_mut().feed(&bytes);
            if let Some(ui) = weak.upgrade() {
                refresh(&ui, &terminal.borrow());
            }
        }
    });
    ui.on_start_desktop({
        let weak = ui.as_weak();
        let runtime = runtime.clone();
        let input_tx = input_tx.clone();
        let desktop_stop = desktop_stop.clone();
        let receivers = receivers.clone();
        let jobs = jobs.clone();
        let start_render = start_render.clone();
        let stop = stop.clone();
        move || {
            stop();
            let (inputs_tx, inputs) = mpsc::channel(32);
            *input_tx.borrow_mut() = Some(inputs_tx);
            let (stop_tx, stop_rx) = watch::channel(false);
            *desktop_stop.borrow_mut() = Some(stop_tx);
            let (frames, receiver) = watch::channel(rfb::Frame::default());
            receivers.borrow_mut().0 = receiver;
            let error_ui = weak.clone();
            jobs.borrow_mut().push(runtime.spawn(async move {
                let result = fixture::run_desktop(frames, inputs, stop_rx).await;
                if let Err(error) = &result {
                    let text = error.to_string();
                    let _ = error_ui.upgrade_in_event_loop(move |ui| ui.set_status(text.into()));
                }
                result
            }));
            start_render();
            if let Some(ui) = weak.upgrade() {
                ui.set_status("本机 RFB 图样".into());
            }
        }
    });
    ui.on_pointer({
        let inputs = input_tx.clone();
        move |x, y, buttons| {
            if let Some(inputs) = inputs.borrow().as_ref() {
                let _ = inputs.try_send(vnc::X11Event::PointerEvent(
                    (
                        x.clamp(0, 319) as u16,
                        y.clamp(0, 179) as u16,
                        buttons as u8,
                    )
                        .into(),
                ));
            }
        }
    });
    ui.on_desktop_key({
        let inputs = input_tx.clone();
        let weak = ui.as_weak();
        move |text, down| {
            if let Some(inputs) = inputs.borrow().as_ref() {
                for c in text.chars() {
                    if let Err(error) =
                        inputs.try_send(vnc::X11Event::KeyEvent((rfb::keysym(c), down).into()))
                    {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_status(format!("输入未发送：{error}").into());
                        }
                    }
                }
            }
        }
    });
    ui.on_start_video({
        let weak = ui.as_weak();
        let flags = video_stop.clone();
        let jobs = video_jobs.clone();
        let tx = video_tx.clone();
        let count = video_count.clone();
        let start_render = start_render.clone();
        let stop = stop.clone();
        move || {
            stop();
            let Some(path) = std::env::var_os("PORTCANVAS_PROBE_VIDEO").map(PathBuf::from) else {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status("未指定本地测试视频".into());
                }
                return;
            };
            let stopped = Arc::new(AtomicBool::new(false));
            *flags.borrow_mut() = Some(stopped.clone());
            let frames = tx.clone();
            let count = count.clone();
            let error_ui = weak.clone();
            jobs.borrow_mut()
                .push(std::thread::spawn(move || -> Result<()> {
                    let result: Result<()> = (|| {
                        while !stopped.load(Ordering::SeqCst) {
                            let mut reader = media::Reader::open(&path)?;
                            let clock = Instant::now();
                            while !stopped.load(Ordering::SeqCst) {
                                let Some(frame) = reader.next_frame()? else {
                                    break;
                                };
                                let deadline =
                                    Duration::from_nanos(frame.timestamp_100ns.max(0) as u64 * 100);
                                if let Some(delay) = deadline.checked_sub(clock.elapsed()) {
                                    std::thread::sleep(delay.min(Duration::from_millis(100)));
                                }
                                frames.send_replace(Some(frame));
                                count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Ok(())
                    })();
                    stopped.store(true, Ordering::SeqCst);
                    if let Err(error) = &result {
                        let text = error.to_string();
                        let _ =
                            error_ui.upgrade_in_event_loop(move |ui| ui.set_status(text.into()));
                    }
                    result
                }));
            start_render();
            if let Some(ui) = weak.upgrade() {
                ui.set_status("Windows 系统视频解码 · 无音轨测试素材".into());
            }
        }
    });
    let outcome = ui.run();
    stop();
    timer.stop();
    let mut errors = Vec::new();
    for job in jobs.borrow_mut().drain(..) {
        match runtime.block_on(job) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e.to_string()),
            Err(e) => errors.push(e.to_string()),
        }
    }
    for job in video_jobs.borrow_mut().drain(..) {
        match job.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e.to_string()),
            Err(_) => errors.push("media worker panicked".into()),
        }
    }
    if let Some(path) = std::env::var_os("PORTCANVAS_PROBE_REPORT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(
                &serde_json::json!({"terminalText":terminal.borrow().text(),"desktopFrames":*desktop_count.borrow(),"videoFrames":video_count.load(Ordering::Relaxed),"lifecycle":*lifecycle.borrow(),"errors":errors}),
            )?,
        )?;
    }
    outcome?;
    Ok(())
}
