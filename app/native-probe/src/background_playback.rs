//! File-backed native playback. One latest video frame, system audio, no HTTP.
use crate::{
    media::{Reader, VideoFrame},
    media_audio::Audio,
};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
struct Controls {
    paused: bool,
    volume: u8,
}
struct Shared {
    controls: Mutex<Controls>,
    stop: Arc<AtomicBool>,
    wake: Condvar,
    state: Mutex<State>,
}
#[derive(Default)]
pub struct State {
    pub frame: Option<Arc<VideoFrame>>,
    pub sequence: u64,
    pub loops: u64,
    pub position: i64,
    pub has_audio: bool,
    pub audio_active: bool,
    pub audio_volume: Option<f32>,
    pub error: Option<String>,
    pub audio_error: Option<String>,
}
pub struct Playback {
    shared: Arc<Shared>,
    job: Option<JoinHandle<()>>,
}
struct Timeline {
    base: i64,
    anchor: Instant,
}
impl Timeline {
    fn now(&self) -> i64 {
        self.base + (self.anchor.elapsed().as_nanos() / 100) as i64
    }
    fn seek(&mut self, value: i64) {
        self.base = value;
        self.anchor = Instant::now();
    }
}
struct Engine {
    audio: Option<Audio>,
    audio_failed: bool,
    level: u8,
    paused: bool,
    timeline: Timeline,
    has_audio: bool,
}
impl Engine {
    fn position(&self) -> i64 {
        self.audio
            .as_ref()
            .filter(|a| !a.ended())
            .and_then(|a| a.time().ok())
            .unwrap_or_else(|| self.timeline.now())
    }
    fn audio_action<T>(
        &mut self,
        shared: &Shared,
        action: impl FnOnce(&Audio) -> Result<T>,
    ) -> Option<T> {
        match self.audio.as_ref().map(action) {
            Some(Ok(value)) => Some(value),
            Some(Err(error)) => {
                let now = self.position();
                self.audio.take();
                self.timeline.seek(now);
                self.audio_failed = true;
                let mut state = shared.state.lock().unwrap();
                state.audio_error = Some(error.to_string());
                state.audio_active = false;
                state.audio_volume = None;
                None
            }
            None => None,
        }
    }
    fn wait(&mut self, path: &Path, shared: &Arc<Shared>, target: i64) -> Result<bool> {
        loop {
            let control = *shared.controls.lock().unwrap();
            if shared.stop.load(Ordering::SeqCst) {
                return Ok(false);
            }
            if let Some(Some(now)) = self.audio_action(shared, |a| {
                if a.pump()? {
                    Ok(Some(a.time()?))
                } else {
                    Ok(None)
                }
            }) {
                self.timeline.seek(now);
            }
            if control.volume == 0 {
                self.audio_failed = false;
                if self.audio.is_some() {
                    let now = self.position();
                    self.audio.take();
                    self.timeline.seek(now);
                    let mut state = shared.state.lock().unwrap();
                    state.audio_active = false;
                    state.audio_volume = None;
                }
            }
            if control.paused {
                if !self.paused {
                    let now = self.position();
                    self.audio_action(shared, |a| a.pause());
                    self.timeline.seek(now);
                    self.paused = true;
                    shared.state.lock().unwrap().position = now;
                }
                let lock = shared.controls.lock().unwrap();
                if lock.paused && !shared.stop.load(Ordering::SeqCst) {
                    drop(shared.wake.wait(lock).unwrap());
                }
                continue;
            }
            if self.paused {
                self.timeline.seek(self.timeline.base);
                self.audio_action(shared, |a| a.resume());
                self.paused = false;
            }
            if control.volume > 0 && self.has_audio && !self.audio_failed && self.audio.is_none() {
                let now = self.timeline.now();
                match Audio::open(path, now, control.volume, shared.stop.clone()) {
                    Ok(audio) => {
                        self.audio_failed = audio.is_none();
                        self.audio = audio;
                        self.timeline.seek(now);
                        shared.state.lock().unwrap().audio_error = None;
                    }
                    Err(error) => {
                        if shared.stop.load(Ordering::SeqCst) {
                            return Ok(false);
                        }
                        shared.state.lock().unwrap().audio_error = Some(error.to_string());
                        self.audio_failed = true;
                    }
                }
            }
            if control.volume != self.level {
                self.audio_action(shared, |a| a.set_volume(control.volume));
                self.level = control.volume;
            }
            let now = self.position();
            {
                let mut state = shared.state.lock().unwrap();
                state.position = now;
                state.audio_active = self.audio.is_some();
                state.audio_volume = self.audio.as_ref().and_then(|a| a.volume().ok());
            }
            if now >= target {
                return Ok(true);
            }
            let lock = shared.controls.lock().unwrap();
            let delay = Duration::from_nanos(((target - now) as u64 * 100).min(20_000_000));
            drop(shared.wake.wait_timeout(lock, delay).unwrap());
        }
    }
}
impl Playback {
    pub fn open(path: PathBuf, volume: u8, paused: bool) -> Self {
        let shared = Arc::new(Shared {
            controls: Mutex::new(Controls { paused, volume }),
            stop: Arc::new(AtomicBool::new(false)),
            wake: Condvar::new(),
            state: Mutex::new(State::default()),
        });
        let worker = shared.clone();
        let job = std::thread::spawn(move || {
            let result = run(&path, &worker);
            let mut state = worker.state.lock().unwrap();
            state.audio_active = false;
            if !worker.stop.load(Ordering::SeqCst) {
                if let Err(error) = result {
                    state.error = Some(format!("{error:#}"));
                }
            }
        });
        Self {
            shared,
            job: Some(job),
        }
    }
    pub fn paused(&self, value: bool) {
        self.shared.controls.lock().unwrap().paused = value;
        self.shared.wake.notify_all();
    }
    pub fn volume(&self, value: u8) {
        self.shared.controls.lock().unwrap().volume = value.min(100);
        self.shared.wake.notify_all();
    }
    pub fn read<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        read(&self.shared.state.lock().unwrap())
    }
    pub fn close(&mut self) -> Result<()> {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        if let Some(job) = self.job.take() {
            job.join()
                .map_err(|_| anyhow::anyhow!("视频播放线程异常。"))?;
        }
        Ok(())
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn run(path: &Path, shared: &Arc<Shared>) -> Result<()> {
    let mut reader = Reader::open(path)?;
    let first = reader.next_frame()?.context("视频没有可用画面。")?;
    let has_audio = reader.has_audio();
    let mut last = first.timestamp_100ns;
    let mut duration = first.duration_100ns.unwrap_or(333_333);
    {
        let mut state = shared.state.lock().unwrap();
        state.frame = Some(Arc::new(first));
        state.sequence = 1;
        state.has_audio = has_audio;
    }
    let mut engine = Engine {
        audio: None,
        audio_failed: false,
        level: 0,
        paused: false,
        timeline: Timeline {
            base: 0,
            anchor: Instant::now(),
        },
        has_audio,
    };
    loop {
        if !engine.wait(path, shared, 0)? {
            break;
        }
        if let Some(frame) = reader.next_frame()? {
            if let Some(value) = frame.duration_100ns {
                duration = value;
            } else if frame.timestamp_100ns > last {
                duration = frame.timestamp_100ns - last;
            }
            last = frame.timestamp_100ns;
            if !engine.wait(path, shared, last)? {
                break;
            }
            let mut state = shared.state.lock().unwrap();
            state.frame = Some(Arc::new(frame));
            state.sequence += 1;
        } else {
            if !engine.wait(path, shared, last + duration)? {
                break;
            }
            reader.rewind()?;
            engine.audio_action(shared, |a| a.seek(0));
            engine.timeline.seek(0);
            last = 0;
            shared.state.lock().unwrap().loops += 1;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_loops_pauses_and_releases_its_file() -> Result<()> {
        let root =
            PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join("background-playback");
        std::fs::create_dir_all(&root)?;
        let path = root.join("silent.mp4");
        crate::media::write_fixture(&path)?;
        let mut player = Playback::open(path.clone(), 0, false);
        let end = Instant::now() + Duration::from_secs(12);
        while player.read(|s| s.loops) < 1 && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(player.read(|s| s.error.is_none() && s.loops >= 1));
        player.paused(true);
        std::thread::sleep(Duration::from_millis(100));
        let before = player.read(|s| s.sequence);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(player.read(|s| s.sequence), before);
        player.paused(false);
        std::thread::sleep(Duration::from_millis(150));
        assert!(player.read(|s| s.sequence) > before);
        player.close()?;
        std::fs::remove_file(path)?;
        std::fs::remove_dir(root)?;
        Ok(())
    }
}
