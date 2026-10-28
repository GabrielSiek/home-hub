//! The assistant's ears: the default microphone, recorded until the speaker goes quiet, and made
//! into words by Whisper on the home server (which holds the DeepInfra key). Its voice is
//! speech.rs, on this PC.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::state::AppState;

/// Whisper's own rate; smaller uploads than the mic's 48 kHz.
const WAV_RATE: u32 = 16_000;
/// A chunk louder than this (RMS, full scale 1.0) and well above the room's noise is speech.
const SPEECH_FLOOR: f32 = 0.015;
/// Quiet this long after speaking ends the recording.
const SILENCE_END: Duration = Duration::from_millis(1300);
/// Nothing said this long after the mic opened: give up.
const NO_SPEECH: Duration = Duration::from_secs(8);
const MAX_LENGTH: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(50);

pub enum Stop {
    /// Stop and transcribe what was said so far (mic tapped again, Y released).
    Finish,
    /// Stop and forget it.
    Cancel,
}

/// The recording in progress, if any.
#[derive(Default)]
pub struct Recorder {
    stop: Mutex<Option<Sender<Stop>>>,
}

impl Recorder {
    pub fn finish(&self) {
        self.send(Stop::Finish);
    }

    pub fn cancel(&self) {
        self.send(Stop::Cancel);
    }

    fn send(&self, s: Stop) {
        if let Some(tx) = self.stop.lock().unwrap().take() {
            let _ = tx.send(s);
        }
    }
}

/// Records until the speaker goes quiet (or `Recorder::finish`), then has the server transcribe
/// it. None when it was cancelled or nothing was said.
pub async fn listen(app: &AppHandle) -> Result<Option<String>> {
    let st = app.state::<AppState>();
    let (tx, rx) = mpsc::channel();
    if let Some(previous) = st.assistant.recorder.stop.lock().unwrap().replace(tx) {
        let _ = previous.send(Stop::Cancel);
    }
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    // cpal's stream can't move between threads, so it lives and dies on this one.
    let handle = app.clone();
    std::thread::Builder::new()
        .name("microphone".into())
        .spawn(move || {
            let ready = move || {
                let _ = handle.emit("mic-ready", ());
            };
            let _ = done_tx.send(record(rx, ready));
        })?;
    let wav = done_rx
        .await
        .map_err(|_| Error::msg("The microphone stopped unexpectedly"))??;
    let Some(wav) = wav else {
        return Ok(None);
    };
    let wav = base64::engine::general_purpose::STANDARD.encode(wav);
    let text = crate::agent::server_call(app, "assistant_transcribe", json!({ "wav": wav })).await.map_err(Error::msg)?;
    Ok(text.as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string))
}

/// Whether this PC has a microphone (for the house's list of what each TV PC can do).
pub fn has_microphone() -> bool {
    cpal::default_host().default_input_device().is_some()
}

fn mic_err(e: impl std::fmt::Display) -> Error {
    Error::msg(format!("Microphone: {e}"))
}

/// A 16 kHz mono WAV of what was said; None when cancelled or silent. `ready` runs once
/// sound comes in: some mics take most of a second to open, and words before that are lost.
fn record(stop: Receiver<Stop>, ready: impl FnOnce()) -> Result<Option<Vec<u8>>> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| Error::msg("No microphone found. Plug one in or pick a default input in Windows."))?;
    let supported = device.default_input_config().map_err(mic_err)?;
    let rate = supported.sample_rate().0;
    let channels = usize::from(supported.channels()).max(1);
    let config = supported.config();
    let samples: Arc<Mutex<Vec<f32>>> = Arc::default();
    let s = samples.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => input_stream::<f32>(&device, &config, channels, s),
        SampleFormat::I16 => input_stream::<i16>(&device, &config, channels, s),
        SampleFormat::U16 => input_stream::<u16>(&device, &config, channels, s),
        SampleFormat::I32 => input_stream::<i32>(&device, &config, channels, s),
        f => Err(mic_err(format!("unsupported sample format {f}"))),
    }?;
    stream.play().map_err(mic_err)?;

    let started = Instant::now();
    let mut vad = Vad::default();
    let mut seen = 0;
    let mut speech = None;
    let mut ready = Some(ready);
    loop {
        match stop.recv_timeout(POLL) {
            Ok(Stop::Finish) => break,
            Ok(Stop::Cancel) | Err(RecvTimeoutError::Disconnected) => return Ok(None),
            Err(RecvTimeoutError::Timeout) => {}
        }
        let (level, from) = {
            let s = samples.lock().unwrap();
            let from = seen;
            seen = s.len();
            (rms(&s[from..]), from)
        };
        if level.is_some() {
            if let Some(f) = ready.take() {
                f();
            }
        }
        let verdict = level.map(|l| vad.update(l, started.elapsed()));
        if verdict.is_some() && vad.speaking() {
            speech = Some(speech.map_or((from, seen), |(start, _)| (start, seen)));
        }
        match verdict {
            Some(Verdict::Done) => break,
            Some(Verdict::Nothing) => return Ok(None),
            _ if started.elapsed() > MAX_LENGTH => break,
            _ => {}
        }
    }
    drop(stream);

    let mut samples = std::mem::take(&mut *samples.lock().unwrap());
    // Under 0.3 s: a stray tap, nothing to send.
    if samples.len() < rate as usize * 3 / 10 {
        return Ok(None);
    }
    trim(&mut samples, speech, rate as usize / 2);
    let mut mono = resample(&samples, rate, WAV_RATE);
    normalize(&mut mono);
    Ok(Some(wav(&mono, WAV_RATE)))
}

/// Mixes each frame down to mono f32 into `out`.
fn input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channels: usize,
    out: Arc<Mutex<Vec<f32>>>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut out = out.lock().unwrap();
                out.extend(data.chunks(channels).map(|frame| {
                    frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / frame.len() as f32
                }));
            },
            |e| eprintln!("microphone: {e}"),
            None,
        )
        .map_err(mic_err)
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Continue,
    /// Spoke, then went quiet.
    Done,
    /// Never spoke.
    Nothing,
}

/// Tells speech from room noise by loudness, following the noise level as it goes.
struct Vad {
    noise: f32,
    heard: bool,
    quiet_since: Option<Duration>,
}

impl Default for Vad {
    fn default() -> Self {
        Self {
            noise: SPEECH_FLOOR / 3.0,
            heard: false,
            quiet_since: None,
        }
    }
}

impl Vad {
    /// `level`: RMS of the newest chunk; `at`: time since the mic opened.
    fn update(&mut self, level: f32, at: Duration) -> Verdict {
        // Falls to quiet moments at once, rises slowly, so speech doesn't become "noise".
        self.noise = if level < self.noise {
            level
        } else {
            self.noise * 0.995 + level * 0.005
        };
        if level > SPEECH_FLOOR.max(self.noise * 3.0) {
            self.heard = true;
            self.quiet_since = None;
        } else if self.heard && self.quiet_since.is_none() {
            self.quiet_since = Some(at);
        }
        if self.quiet_since.is_some_and(|q| at.saturating_sub(q) >= SILENCE_END) {
            Verdict::Done
        } else if !self.heard && at >= NO_SPEECH {
            Verdict::Nothing
        } else {
            Verdict::Continue
        }
    }

    /// Whether the newest chunk was speech.
    fn speaking(&self) -> bool {
        self.heard && self.quiet_since.is_none()
    }
}

/// Cuts the quiet before and after `speech` (sample range) down to `margin` samples: Whisper
/// fills long silences with words nobody said ("Obrigado.", "Legendas pela comunidade Amara.org").
fn trim(samples: &mut Vec<f32>, speech: Option<(usize, usize)>, margin: usize) {
    let Some((start, end)) = speech else {
        return;
    };
    samples.truncate(end.saturating_add(margin).min(samples.len()));
    samples.drain(..start.saturating_sub(margin).min(samples.len()));
}

fn rms(s: &[f32]) -> Option<f32> {
    if s.is_empty() {
        return None;
    }
    Some((s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt())
}

/// Low-passed below the new rate's limit first, or a 48 kHz mic's hiss and "s" sounds above
/// 8 kHz fold down over the voice; then linear interpolation.
fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let kernel = low_pass(0.45 * from.min(to) as f32 / from as f32);
    let n = (x.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let a = fir(x, &kernel, j);
            let b = if j + 1 < x.len() { fir(x, &kernel, j + 1) } else { a };
            a + (b - a) * (p - j as f64) as f32
        })
        .collect()
}

/// A windowed-sinc low-pass filter; `cutoff` as a fraction of the sample rate.
fn low_pass(cutoff: f32) -> Vec<f32> {
    use std::f32::consts::PI;
    let half = (5.0 / cutoff).ceil() as i32;
    let mut k: Vec<f32> = (-half..=half)
        .map(|i| {
            let t = i as f32;
            let sinc = if i == 0 { 2.0 * cutoff } else { (2.0 * PI * cutoff * t).sin() / (PI * t) };
            sinc * (0.54 + 0.46 * (PI * t / half as f32).cos())
        })
        .collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    k
}

/// `x` filtered by `kernel`, at sample `j`.
fn fir(x: &[f32], kernel: &[f32], j: usize) -> f32 {
    let half = kernel.len() / 2;
    let first = half.saturating_sub(j);
    let last = kernel.len().min(x.len() + half - j);
    (first..last).map(|t| x[j + t - half] * kernel[t]).sum()
}

/// Brings a quiet recording up to a usable level (at most 10×).
fn normalize(x: &mut [f32]) {
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if peak > 0.0 && peak < 0.5 {
        let gain = (0.9 / peak).min(10.0);
        x.iter_mut().for_each(|v| *v *= gain);
    }
}

/// 16-bit PCM mono WAV.
fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn vad_ends_after_speech_then_silence() {
        let mut v = Vad::default();
        assert_eq!(v.update(0.002, MS(0)), Verdict::Continue);
        assert_eq!(v.update(0.1, MS(500)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(1000)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(2000)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(2300)), Verdict::Done);
    }

    #[test]
    fn vad_pause_mid_sentence_keeps_listening() {
        let mut v = Vad::default();
        v.update(0.1, MS(100));
        v.update(0.003, MS(500));
        assert_eq!(v.update(0.08, MS(1500)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(2000)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(3000)), Verdict::Continue);
        assert_eq!(v.update(0.003, MS(3300)), Verdict::Done);
    }

    #[test]
    fn vad_gives_up_on_silence_and_ignores_steady_noise() {
        let mut v = Vad::default();
        // A noisy room just above the floor: the noise estimate catches up, so it isn't speech.
        let mut t = 0;
        while t < 8000 {
            let verdict = v.update(0.012, MS(t));
            if t < 7950 {
                assert_eq!(verdict, Verdict::Continue, "at {t} ms");
            }
            t += 50;
        }
        assert_eq!(v.update(0.012, MS(8000)), Verdict::Nothing);
    }

    #[test]
    fn resample_lengths() {
        let x: Vec<f32> = (0..48).map(|i| i as f32).collect();
        assert_eq!(resample(&x, 48_000, 16_000).len(), 16);
        assert_eq!(resample(&x, 44_100, 16_000).len(), 17);
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn resample_keeps_the_voice_and_drops_what_would_fold_over_it() {
        let level = |hz: f32| {
            let tone: Vec<f32> = (0..48_000)
                .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / 48_000.0).sin())
                .collect();
            rms(&resample(&tone, 48_000, 16_000)[1000..15_000]).unwrap()
        };
        // A sine's RMS is 0.707.
        assert!(level(1000.0) > 0.68, "{}", level(1000.0));
        // 12 kHz would land on 4 kHz.
        assert!(level(12_000.0) < 0.01, "{}", level(12_000.0));
    }

    #[test]
    fn trim_keeps_speech_and_a_margin() {
        let mut s: Vec<f32> = (0..100).map(|i| i as f32).collect();
        trim(&mut s, Some((40, 60)), 10);
        assert_eq!((s[0], s[s.len() - 1]), (30.0, 69.0));
        let mut s: Vec<f32> = (0..100).map(|i| i as f32).collect();
        trim(&mut s, Some((5, 98)), 10);
        assert_eq!(s.len(), 100);
        trim(&mut s, None, 10);
        assert_eq!(s.len(), 100);
    }

    #[test]
    fn vad_speaking_follows_the_newest_chunk() {
        let mut v = Vad::default();
        v.update(0.002, MS(0));
        assert!(!v.speaking());
        v.update(0.1, MS(50));
        assert!(v.speaking());
        v.update(0.002, MS(100));
        assert!(!v.speaking());
    }

    #[test]
    fn wav_header() {
        let w = wav(&[0.0, 1.0, -1.0], 16_000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
        assert_eq!(w.len(), 50);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }

    /// Opens the real default microphone for 1.5 s: `cargo test -- --ignored real_microphone`.
    #[test]
    #[ignore]
    fn real_microphone() {
        let (tx, rx) = mpsc::channel();
        let t = std::thread::spawn(move || record(rx, || println!("mic ready")));
        std::thread::sleep(Duration::from_millis(1500));
        tx.send(Stop::Finish).unwrap();
        let wav = t.join().unwrap().unwrap().expect("recorded audio");
        assert_eq!(&wav[0..4], b"RIFF");
        println!("{} bytes, {:.1} s", wav.len(), (wav.len() - 44) as f32 / (WAV_RATE as f32 * 2.0));
    }
}
