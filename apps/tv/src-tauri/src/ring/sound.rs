//! The ring: a chime made here and played on the PC's speakers, so it sounds whatever is in
//! front (a game, a browser) and however asleep the launcher's page is.

use std::f64::consts::TAU;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Two short beeps, over and over: a kitchen timer.
    Timer,
    /// A rising four-note chime, from soft to full over a minute.
    Alarm,
}

/// The ring playing now, if any. Its thread owns the audio stream (cpal's can't move between
/// threads) and ends when its sender is dropped.
#[derive(Default)]
pub struct Bell {
    playing: Mutex<Option<(Tone, Sender<()>)>>,
}

impl Bell {
    /// Rings `tone` until `stop`; already ringing another tone, it switches.
    pub fn ring(&self, tone: Tone) {
        let mut playing = self.playing.lock().unwrap();
        if playing.as_ref().is_some_and(|(t, _)| *t == tone) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        // Replacing the sender ends the other tone's thread.
        *playing = Some((tone, tx));
        let _ = std::thread::Builder::new().name("bell".into()).spawn(move || {
            if let Err(e) = play(tone, &rx) {
                eprintln!("clock: the bell: {e}");
            }
        });
    }

    pub fn stop(&self) {
        self.playing.lock().unwrap().take();
    }
}

fn play(tone: Tone, until: &Receiver<()>) -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no speakers found")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let synth = Synth { tone, rate: f64::from(supported.sample_rate().0), n: 0 };
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, &config, synth),
        SampleFormat::I16 => stream::<i16>(&device, &config, synth),
        SampleFormat::U16 => stream::<u16>(&device, &config, synth),
        SampleFormat::I32 => stream::<i32>(&device, &config, synth),
        f => Err(format!("unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    // Nothing is ever sent: this returns once the sender is gone.
    let _ = until.recv();
    Ok(())
}

fn stream<T>(device: &cpal::Device, config: &StreamConfig, mut synth: Synth) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let v = T::from_sample(synth.next());
                    frame.fill(v);
                }
            },
            |e| eprintln!("clock: the bell: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

struct Synth {
    tone: Tone,
    rate: f64,
    /// Samples played so far.
    n: u64,
}

impl Synth {
    fn next(&mut self) -> f32 {
        // Seconds in f64: in f32 the sine's phase would get coarse after a few minutes.
        let t = self.n as f64 / self.rate;
        self.n += 1;
        (match self.tone {
            Tone::Timer => timer(t),
            Tone::Alarm => alarm(t),
        }) as f32
    }
}

fn timer(t: f64) -> f64 {
    let p = t % 1.2;
    let beep = |at: f64| {
        let x = p - at;
        if !(0.0..0.14).contains(&x) {
            return 0.0;
        }
        // 5 ms in, 20 ms out: no clicks.
        let env = (x / 0.005).min(1.0) * ((0.14 - x) / 0.02).min(1.0);
        env * ((TAU * 1318.5 * x).sin() * 0.8 + (TAU * 2637.0 * x).sin() * 0.2)
    };
    (beep(0.0) + beep(0.22)) * 0.45
}

/// C6, E6, G6, C7.
const CHIME: [(f64, f64); 4] = [(0.0, 1046.5), (0.22, 1318.5), (0.44, 1568.0), (0.66, 2093.0)];

fn alarm(t: f64) -> f64 {
    let p = t % 2.4;
    let notes: f64 = CHIME
        .iter()
        .map(|(at, f)| {
            let x = p - at;
            if !(0.0..1.7).contains(&x) {
                return 0.0;
            }
            // A struck bell: quick attack, long decay, an octave that fades sooner.
            let env = (x / 0.004).min(1.0) * (-x * 3.0).exp() * ((1.7 - x) / 0.05).min(1.0);
            env * ((TAU * f * x).sin() + 0.35 * (TAU * 2.0 * f * x).sin() * (-x * 6.0).exp())
        })
        .sum();
    let louder = 0.2 + 0.8 * (t / 60.0).min(1.0);
    notes * 0.24 * louder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_stay_in_range_and_rise() {
        let peak = |f: fn(f64) -> f64, from: f64, to: f64| {
            (0..((to - from) * 8000.0) as usize)
                .map(|i| f(from + i as f64 / 8000.0).abs())
                .fold(0.0, f64::max)
        };
        assert!(peak(timer, 0.0, 2.4) <= 1.0);
        assert!(peak(alarm, 60.0, 62.4) <= 1.0);
        // Softer at first, full after a minute.
        assert!(peak(alarm, 0.0, 2.4) < peak(alarm, 60.0, 62.4) / 2.0);
        // The gap between beeps is silent.
        assert_eq!(timer(0.5), 0.0);
    }
}
