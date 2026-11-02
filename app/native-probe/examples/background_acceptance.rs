use anyhow::{ensure, Result};
use portcanvas_native_probe::{
    background_playback::Playback,
    media::{self, Reader},
    media_audio::Audio,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn main() -> Result<()> {
    let root = PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap());
    std::fs::create_dir_all(&root)?;
    let path = root.join("audio-video.mp4");
    media::write_av_fixture(&path)?;
    ensure!(Reader::open(&path)?.has_audio());
    let mut p = Playback::open(path.clone(), 25, false);
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end
        && !p.read(|s| s.audio_active && s.position > 5_000_000 && s.loops >= 2)
    {
        if let Some(e) = p.read(|s| s.error.clone().or(s.audio_error.clone())) {
            anyhow::bail!("{e}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ensure!(
        p.read(|s| s.loops >= 2),
        "Audio/video loop did not complete twice"
    );
    ensure!(
        p.read(|s| s.audio_active && s.audio_volume == Some(0.25)),
        "Audio session did not become active"
    );
    p.paused(true);
    std::thread::sleep(Duration::from_millis(100));
    let before = p.read(|s| (s.sequence, s.position));
    std::thread::sleep(Duration::from_millis(150));
    ensure!(p.read(|s| (s.sequence, s.position)) == before);
    p.volume(0);
    std::thread::sleep(Duration::from_millis(100));
    ensure!(!p.read(|s| s.audio_active));
    p.paused(false);
    p.close()?;
    let audio = Audio::open(
        &path,
        0,
        25,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )?
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !audio.ended() && Instant::now() < deadline {
        audio.pump()?;
        std::thread::sleep(Duration::from_millis(10));
    }
    ensure!(audio.ended(), "Audio did not reach its natural end");
    audio.pause()?;
    audio.resume()?;
    audio.seek(0)?;
    ensure!(
        !audio.ended(),
        "Audio failed to restart after its natural end"
    );
    drop(audio);
    let mut webm = None;
    if let Some(path) = std::env::var_os("PORTCANVAS_WEBM_FIXTURE") {
        let path = PathBuf::from(path);
        let mut r = Reader::open(&path)?;
        let frame = r.next_frame()?.unwrap();
        let has_audio = r.has_audio();
        drop(r);
        let mut player = Playback::open(path, 25, false);
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end
            && !player.read(|s| s.position > 5_000_000 && (!has_audio || s.audio_active))
        {
            if let Some(e) = player.read(|s| s.error.clone().or(s.audio_error.clone())) {
                anyhow::bail!("WebM: {e}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        ensure!(player.read(|s| !has_audio || s.audio_active));
        player.close()?;
        webm = Some(
            serde_json::json!({"width":frame.width,"height":frame.height,"audio":has_audio,"nativePlayback":true}),
        );
    }
    std::fs::write(
        root.join("playback-acceptance.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"audioSession":true,"volumeReadback":0.25,"pausedStable":true,"mutedAudioReleased":true,"naturalEndPauseRestart":true,"webm":webm}),
        )?,
    )?;
    println!("Native audio, pause, volume and decoder checks passed.");
    Ok(())
}
