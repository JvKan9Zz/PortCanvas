#![cfg(target_os="macos")]
use portcanvas_native_probe::{media, background_playback::Playback};
use std::{path::PathBuf,time::{Duration,Instant}};
#[test]
fn file_decoder_rewinds_and_playback_pauses_without_retaining_the_file() -> anyhow::Result<()> {
    let root=PathBuf::from(std::env::var_os("WCS_TEST_ROOT").unwrap()).join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&root)?;let path=root.join("fixture.mp4");media::write_fixture(&path)?;
    let mut reader=media::Reader::open(&path)?;
    assert!(!reader.has_audio());let first=reader.next_frame()?.unwrap();
    assert_eq!((first.width,first.height),(320,180));assert_eq!(first.rgba.len(),320*180*4);
    while reader.next_frame()?.is_some() {} reader.rewind()?;assert!(reader.next_frame()?.is_some());drop(reader);
    let mut player=Playback::open(path.clone(),0,false);let deadline=Instant::now()+Duration::from_secs(10);
    while player.read(|s|s.loops)<1 && Instant::now()<deadline {std::thread::sleep(Duration::from_millis(20));}
    assert!(player.read(|s|s.error.is_none()&&s.loops>=1));player.paused(true);
    std::thread::sleep(Duration::from_millis(100));let sequence=player.read(|s|s.sequence);
    std::thread::sleep(Duration::from_millis(150));assert_eq!(sequence,player.read(|s|s.sequence));
    player.paused(false);std::thread::sleep(Duration::from_millis(150));assert!(player.read(|s|s.sequence)>sequence);
    player.close()?;std::fs::remove_dir_all(root)?;Ok(())
}
