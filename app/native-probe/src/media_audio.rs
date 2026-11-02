//! An audio-only Media Foundation topology with per-session volume.
use crate::media::{wide, Environment};
use anyhow::{ensure, Result};
use std::{
    cell::Cell,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{Interface, GUID, PCWSTR},
    Win32::{Media::MediaFoundation::*, System::Com::StructuredStorage::PROPVARIANT},
};
struct Source(IMFMediaSource);
impl Drop for Source {
    fn drop(&mut self) {
        unsafe {
            let _ = self.0.Shutdown();
        }
    }
}
pub struct Audio {
    session: IMFMediaSession,
    source: Source,
    volume: Option<IMFSimpleAudioVolume>,
    cancel: Arc<AtomicBool>,
    ended: Cell<bool>,
    _environment: Environment,
}
impl Drop for Audio {
    fn drop(&mut self) {
        unsafe {
            let _ = self.session.Close();
            let _ = self.session.Shutdown();
        }
    }
}
impl Audio {
    pub fn open(
        path: &Path,
        position: i64,
        level: u8,
        cancel: Arc<AtomicBool>,
    ) -> Result<Option<Self>> {
        let environment = Environment::new()?;
        unsafe {
            let resolver = MFCreateSourceResolver()?;
            let mut kind = MF_OBJECT_INVALID;
            let mut object = None;
            let path = wide(path);
            resolver.CreateObjectFromURL(
                PCWSTR(path.as_ptr()),
                MF_RESOLUTION_MEDIASOURCE.0 as u32,
                None,
                &mut kind,
                &mut object,
            )?;
            let source = Source(object.unwrap().cast()?);
            let descriptor = source.0.CreatePresentationDescriptor()?;
            let mut audio = None;
            for index in 0..descriptor.GetStreamDescriptorCount()? {
                let mut selected = false.into();
                let mut stream = None;
                descriptor.GetStreamDescriptorByIndex(index, &mut selected, &mut stream)?;
                let stream = stream.unwrap();
                if audio.is_none()
                    && stream.GetMediaTypeHandler()?.GetMajorType()? == MFMediaType_Audio
                {
                    descriptor.SelectStream(index)?;
                    audio = Some(stream);
                } else {
                    descriptor.DeselectStream(index)?;
                }
            }
            let Some(stream) = audio else {
                return Ok(None);
            };
            let session = MFCreateMediaSession(None)?;
            let mut result = Self {
                session,
                source,
                volume: None,
                cancel,
                ended: Cell::new(false),
                _environment: environment,
            };
            let topology = MFCreateTopology()?;
            let input = MFCreateTopologyNode(MF_TOPOLOGY_SOURCESTREAM_NODE)?;
            input.SetUnknown(&MF_TOPONODE_SOURCE, &result.source.0)?;
            input.SetUnknown(&MF_TOPONODE_PRESENTATION_DESCRIPTOR, &descriptor)?;
            input.SetUnknown(&MF_TOPONODE_STREAM_DESCRIPTOR, &stream)?;
            let output = MFCreateTopologyNode(MF_TOPOLOGY_OUTPUT_NODE)?;
            let renderer = MFCreateAudioRendererActivate()?;
            output.SetObject(&renderer)?;
            topology.AddNode(&input)?;
            topology.AddNode(&output)?;
            input.ConnectOutput(0, &output, 0)?;
            result.session.SetTopology(0, &topology)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                ensure!(!result.cancel.load(Ordering::SeqCst), "音频播放已取消。");
                ensure!(Instant::now() < deadline, "音频输出准备超时。");
                if let Ok(event) = result.session.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    event.GetStatus()?.ok()?;
                    if event.GetType()? == MESessionTopologyStatus.0 as u32
                        && event.GetUINT32(&MF_EVENT_TOPOLOGY_STATUS)?
                            == MF_TOPOSTATUS_READY.0 as u32
                    {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            result.volume = Some(
                result
                    .session
                    .cast::<IMFGetService>()?
                    .GetService(&MR_POLICY_VOLUME_SERVICE)?,
            );
            result.set_volume(level)?;
            result.seek(position)?;
            Ok(Some(result))
        }
    }
    fn event(&self) -> Result<Option<u32>> {
        unsafe {
            match self.session.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                Ok(event) => {
                    event.GetStatus()?.ok()?;
                    let kind = event.GetType()?;
                    if kind == MESessionEnded.0 as u32 {
                        self.ended.set(true);
                    }
                    Ok(Some(kind))
                }
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
    }
    fn wait_event(&self, kind: u32) -> Result<()> {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            ensure!(!self.cancel.load(Ordering::SeqCst), "音频播放已取消。");
            ensure!(Instant::now() < end, "音频状态切换超时。");
            if self.event()? == Some(kind) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn pump(&self) -> Result<bool> {
        let previous = self.ended.get();
        while self.event()?.is_some() {}
        Ok(!previous && self.ended.get())
    }
    pub fn ended(&self) -> bool {
        self.ended.get()
    }
    pub fn seek(&self, position: i64) -> Result<()> {
        self.pump()?;
        self.ended.set(false);
        unsafe {
            let position: PROPVARIANT = position.into();
            self.session.Start(&GUID::zeroed(), &position)?;
        }
        self.wait_event(MESessionStarted.0 as u32)
    }
    pub fn pause(&self) -> Result<()> {
        self.pump()?;
        if self.ended() {
            return Ok(());
        }
        unsafe {
            self.session.Pause()?;
        }
        self.wait_event(MESessionPaused.0 as u32)
    }
    pub fn resume(&self) -> Result<()> {
        self.pump()?;
        if self.ended() {
            return Ok(());
        }
        unsafe {
            self.session
                .Start(&GUID::zeroed(), &PROPVARIANT::default())?;
        }
        self.wait_event(MESessionStarted.0 as u32)
    }
    pub fn time(&self) -> Result<i64> {
        unsafe {
            Ok(self
                .session
                .GetClock()?
                .cast::<IMFPresentationClock>()?
                .GetTime()?)
        }
    }
    pub fn set_volume(&self, level: u8) -> Result<()> {
        unsafe {
            self.volume
                .as_ref()
                .unwrap()
                .SetMasterVolume(f32::from(level.min(100)) / 100.)?;
        }
        Ok(())
    }
    pub fn volume(&self) -> Result<f32> {
        unsafe { Ok(self.volume.as_ref().unwrap().GetMasterVolume()?) }
    }
}
