//! Microphone capture in the engine process (cpal over ALSA; on PipeWire and
//! PulseAudio desktops the default ALSA device is the desktop's default
//! microphone). The window never touches the microphone: WebKitGTK's
//! getUserMedia is off by default, the Tauri webview denies its permission
//! requests, and it would need GStreamer PipeWire plugins inside the AppImage.
//!
//! A cpal stream must stay on the thread that built it, so each recording
//! owns a small thread that holds the stream until `finish` or drop.
use super::audio;
use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// What the audio callback writes and the status poll reads.
pub struct Shared {
    samples: Mutex<Vec<f32>>,
    rate: AtomicU32,
    /// Latest meter value (f32 bits), 0–1.
    level: AtomicU32,
    /// The recording reached its length limit and stopped taking audio.
    full: AtomicBool,
    error: Mutex<Option<String>>,
    started: Instant,
    max_samples: AtomicU32,
}

impl Shared {
    /// The rate and length limit are set once the device is open.
    fn new() -> Self {
        Self {
            samples: Mutex::new(Vec::new()),
            rate: AtomicU32::new(0),
            level: AtomicU32::new(0),
            full: AtomicBool::new(false),
            error: Mutex::new(None),
            started: Instant::now(),
            max_samples: AtomicU32::new(0),
        }
    }

    /// Append one callback's interleaved frames.
    pub fn push(&self, interleaved: &[f32], channels: usize) {
        let mono = audio::downmix(interleaved, channels);
        self.level
            .store(audio::meter(audio::rms(&mono)).to_bits(), Ordering::Relaxed);
        let limit = self.max_samples.load(Ordering::Relaxed) as usize;
        if let Ok(mut samples) = self.samples.lock() {
            let room = limit.saturating_sub(samples.len());
            if mono.len() > room {
                self.full.store(true, Ordering::Relaxed);
            }
            samples.extend_from_slice(&mono[..mono.len().min(room)]);
        }
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }
    pub fn full(&self) -> bool {
        self.full.load(Ordering::Relaxed)
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
    /// Seconds of audio captured so far.
    pub fn seconds(&self) -> f32 {
        let rate = self.rate();
        if rate == 0 {
            return 0.0;
        }
        self.samples.lock().map(|s| s.len()).unwrap_or(0) as f32 / rate as f32
    }
    /// A copy of the last `seconds` of audio (for the live preview).
    pub fn tail(&self, seconds: f32) -> Vec<f32> {
        let keep = (seconds * self.rate() as f32) as usize;
        self.samples
            .lock()
            .map(|s| s[s.len().saturating_sub(keep)..].to_vec())
            .unwrap_or_default()
    }
    fn take(&self) -> Vec<f32> {
        self.samples
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default()
    }
}

pub struct Recorder {
    pub shared: Arc<Shared>,
    pub device: String,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Open the default input device and start recording. Fails fast when
    /// there is no microphone or it cannot be opened.
    pub fn start(max_seconds: u32) -> Result<Self> {
        let shared = Arc::new(Shared::new());
        let (stop, stopped) = mpsc::channel::<()>();
        let (ready_tx, ready) = mpsc::channel::<Result<String>>();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("shadowcode-mic".into())
            .spawn(move || {
                let stream = match open(thread_shared.clone(), max_seconds) {
                    Ok((stream, name)) => {
                        let _ = ready_tx.send(Ok(name));
                        stream
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                // Hold the stream until told to stop (or the owner is gone).
                let _ = stopped.recv();
                let _ = stream.pause();
                drop(stream);
            })
            .context("Could not start the recording thread")?;
        let device = ready
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| anyhow!("The microphone did not start within 5 seconds"))??;
        Ok(Self {
            shared,
            device,
            stop,
            thread: Some(thread),
        })
    }

    /// Stop and return mono samples with their rate.
    pub fn finish(mut self) -> (Vec<f32>, u32) {
        self.close();
        (self.shared.take(), self.shared.rate())
    }

    fn close(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.close();
    }
}

fn open(shared: Arc<Shared>, max_seconds: u32) -> Result<(cpal::Stream, String)> {
    let host = cpal::default_host();
    let device = host.default_input_device().context(
        "No microphone found. Connect one, or choose it as the default input in your sound settings.",
    )?;
    let name = device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| "Default microphone".into());
    let supported = device
        .default_input_config()
        .map_err(|e| anyhow!("Could not read the microphone's settings: {e}"))?;
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate();
    shared.rate.store(rate, Ordering::Relaxed);
    shared
        .max_samples
        .store(rate.saturating_mul(max_seconds), Ordering::Relaxed);
    let config = supported.config();
    let errors = shared.clone();
    let on_error = move |error: cpal::Error| {
        if let Ok(mut slot) = errors.error.lock() {
            slot.get_or_insert_with(|| format!("The microphone stopped: {error}"));
        }
    };
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let s = shared.clone();
            device.build_input_stream(
                config,
                move |data: &[f32], _: &_| s.push(data, channels),
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let s = shared.clone();
            device.build_input_stream(
                config,
                move |data: &[i16], _: &_| {
                    let converted: Vec<f32> = data.iter().map(|&v| audio::i16_to_f32(v)).collect();
                    s.push(&converted, channels)
                },
                on_error,
                None,
            )
        }
        cpal::SampleFormat::I32 => {
            let s = shared.clone();
            device.build_input_stream(
                config,
                move |data: &[i32], _: &_| {
                    let converted: Vec<f32> =
                        data.iter().map(|&v| v as f32 / 2_147_483_648.0).collect();
                    s.push(&converted, channels)
                },
                on_error,
                None,
            )
        }
        cpal::SampleFormat::U8 => {
            let s = shared.clone();
            device.build_input_stream(
                config,
                move |data: &[u8], _: &_| {
                    let converted: Vec<f32> =
                        data.iter().map(|&v| (v as f32 - 128.0) / 128.0).collect();
                    s.push(&converted, channels)
                },
                on_error,
                None,
            )
        }
        other => {
            return Err(anyhow!(
                "The microphone uses an unsupported format ({other})"
            ))
        }
    }
    .map_err(|e| anyhow!("Could not open the microphone: {e}"))?;
    stream
        .play()
        .map_err(|e| anyhow!("Could not start the microphone: {e}"))?;
    Ok((stream, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_buffer_downmixes_meters_and_stops_at_the_limit() {
        let shared = Shared::new();
        shared.rate.store(4, Ordering::Relaxed);
        shared.max_samples.store(6, Ordering::Relaxed);
        shared.push(&[0.5, 0.5, 0.5, 0.5, 0.5, 0.5], 2);
        assert_eq!(shared.seconds(), 0.75);
        assert!(shared.level() > 0.8);
        assert!(!shared.full());
        shared.push(&[0.0; 8], 2);
        assert!(shared.full(), "the limit was reached");
        assert_eq!(shared.tail(0.5), vec![0.0, 0.0]);
        assert_eq!(shared.level(), 0.0);
        assert_eq!(shared.take().len(), 6);
        assert_eq!(shared.seconds(), 0.0);
    }

    /// Records one second from the real default microphone.
    #[test]
    #[ignore = "needs a microphone"]
    fn records_from_the_default_microphone() {
        let recorder = Recorder::start(5).unwrap();
        std::thread::sleep(Duration::from_secs(1));
        let device = recorder.device.clone();
        let (samples, rate) = recorder.finish();
        eprintln!("{device}: {} samples at {rate} Hz", samples.len());
        assert!(rate >= 8_000);
        assert!(samples.len() as u32 > rate / 2);
    }
}
