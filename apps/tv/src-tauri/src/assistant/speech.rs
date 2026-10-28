//! The assistant's voice, made on this PC: Kokoro (the model DeepInfra ran) through sherpa-onnx.
//! Replies are read out on the speakers sentence by sentence while the next one is being made,
//! so the voice starts ~0.7 s after the text instead of after a round trip for all of it. The
//! model (~350 MB) is downloaded once into the data folder; until it's there, the UI falls back
//! to Windows' voices.
//!
//! Measured on the Ryzen 5 7640HS for an 8.8 s reply: fp32 makes it in 3.1 s with the first
//! sentence at 0.7 s; the int8 model is slower than real time (9.6 s, first sentence at 2.2 s).

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::Serialize;
use serde_json::json;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsModelConfig};
use tauri::{AppHandle, Emitter, Manager};

use crate::config::Settings;
use crate::error::{Error, Result};
use crate::state::AppState;

/// sherpa-onnx's packaging of Kokoro v1.0: 54 voices, espeak-ng data for the languages
/// without a lexicon (Portuguese among them).
const MODEL: &str = "kokoro-multi-lang-v1_0";
const MODEL_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2";
/// What a working model folder has.
const MODEL_FILES: [&str; 6] = ["model.onnx", "voices.bin", "tokens.txt", "espeak-ng-data", "dict", "lexicon-us-en.txt"];

/// voices.bin's speakers in id order, as the model's own metadata (speaker_names) lists them.
const VOICES: [&str; 54] = [
    "af_alloy", "af_aoede", "af_bella", "af_heart", "af_jessica", "af_kore", "af_nicole", "af_nova", "af_river",
    "af_sarah", "af_sky", "am_adam", "am_echo", "am_eric", "am_fenrir", "am_liam", "am_michael", "am_onyx",
    "am_puck", "am_santa", "bf_alice", "bf_emma", "bf_isabella", "bf_lily", "bm_daniel", "bm_fable", "bm_george",
    "bm_lewis", "ef_dora", "em_alex", "ff_siwis", "hf_alpha", "hf_beta", "hm_omega", "hm_psi", "if_sara",
    "im_nicola", "jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo", "pf_dora", "pm_alex",
    "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi", "zm_yunjian", "zm_yunxi", "zm_yunxia",
    "zm_yunyang", "em_santa",
];

/// Silence after the last sentence, so the speakers' buffer plays out before the stream closes.
const TAIL: Duration = Duration::from_millis(150);
const POLL: Duration = Duration::from_millis(20);
/// The speakers took nothing for this long: the device went away.
const STALLED: Duration = Duration::from_secs(3);
/// Progress events while downloading, at most this often.
const TICK: Duration = Duration::from_millis(250);
/// Unused this long, the model is let go (it holds ~400 MB); opening the assistant loads it again.
const IDLE: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct Speaker {
    /// Loaded when the assistant opens (about two seconds) and kept while in use. Its inner lock
    /// keeps to one reply at a time.
    engine: Mutex<Option<Arc<Mutex<OfflineTts>>>>,
    used: Mutex<Option<Instant>>,
    /// Set to stop the reply being read out.
    current: Mutex<Option<Arc<AtomicBool>>>,
    /// The download in progress, or the last one's failure.
    job: Mutex<Option<VoiceModel>>,
}

/// The model's download, for Settings ("voice-model" events).
#[derive(Debug, Clone, Serialize)]
pub struct VoiceModel {
    pub state: ModelState,
    /// Bytes downloaded of `total` (0 when unknown).
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelState {
    Missing,
    Downloading,
    Unpacking,
    Ready,
    Failed,
}

impl VoiceModel {
    fn new(state: ModelState) -> Self {
        Self { state, done: 0, total: 0, error: None }
    }

    fn busy(&self) -> bool {
        matches!(self.state, ModelState::Downloading | ModelState::Unpacking)
    }
}

fn model_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("voice").join(MODEL)
}

fn installed(dir: &Path) -> bool {
    MODEL_FILES.iter().all(|f| dir.join(f).exists())
}

pub fn status(st: &AppState) -> VoiceModel {
    let job = st.assistant.speaker.job.lock().unwrap().clone();
    match job {
        Some(j) if j.busy() => j,
        _ if installed(&model_dir(&st.data_dir)) => VoiceModel::new(ModelState::Ready),
        Some(j) => j,
        None => VoiceModel::new(ModelState::Missing),
    }
}

fn set_job(app: &AppHandle, job: Option<VoiceModel>) {
    let st = app.state::<AppState>();
    *st.assistant.speaker.job.lock().unwrap() = job;
    let _ = app.emit("voice-model", status(&st));
}

/// Gets the voice ready for the next reply: loads the model, or downloads it first.
pub async fn prepare(app: AppHandle) {
    if !installed(&model_dir(&app.state::<AppState>().data_dir)) {
        install(app).await;
        return;
    }
    let a = app.clone();
    let loaded = tauri::async_runtime::spawn_blocking(move || engine(&a.state::<AppState>()).map(drop)).await;
    if let Ok(Err(e)) = loaded {
        eprintln!("voice: {e}");
    }
    keep_warm(&app);
}

/// Marks the model as used now; unused for IDLE after this, it's let go.
fn keep_warm(app: &AppHandle) {
    *app.state::<AppState>().assistant.speaker.used.lock().unwrap() = Some(Instant::now());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(IDLE).await;
        let speaker = &app.state::<AppState>().assistant.speaker;
        if speaker.used.lock().unwrap().is_some_and(|t| t.elapsed() >= IDLE) {
            // Busy loading: it's in use after all.
            if let Ok(mut engine) = speaker.engine.try_lock() {
                if engine.take().is_some() {
                    eprintln!("voice: let the model go after {IDLE:?} unused");
                }
            }
        }
    });
}

/// Downloads and unpacks the model, unless it's there or already on its way.
async fn install(app: AppHandle) {
    let st = app.state::<AppState>();
    {
        let mut job = st.assistant.speaker.job.lock().unwrap();
        if job.as_ref().is_some_and(VoiceModel::busy) || installed(&model_dir(&st.data_dir)) {
            return;
        }
        *job = Some(VoiceModel::new(ModelState::Downloading));
    }
    let _ = app.emit("voice-model", status(&st));
    let mut last = Instant::now();
    let result = download(&st.data_dir.join("voice"), |state, done, total| {
        // Each new stage goes out at once, progress within one every TICK.
        if state == ModelState::Downloading && last.elapsed() < TICK {
            return;
        }
        last = Instant::now();
        set_job(&app, Some(VoiceModel { state, done, total, error: None }));
    })
    .await;
    match result {
        Ok(()) => {
            set_job(&app, None);
            // Loaded now, so the first reply doesn't wait for it.
            let a = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || engine(&a.state::<AppState>()).map(drop)).await;
            keep_warm(&app);
        }
        Err(e) => {
            eprintln!("voice: the download failed: {e}");
            set_job(&app, Some(VoiceModel { error: Some(e.to_string()), ..VoiceModel::new(ModelState::Failed) }));
        }
    }
}

/// Fetches the model's archive into `root` and unpacks it to root\<MODEL>. `on(stage, done,
/// total)` hears the progress.
async fn download(root: &Path, mut on: impl FnMut(ModelState, u64, u64)) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    fs::create_dir_all(root)?;
    let archive = root.join(format!(".{MODEL}.tar.bz2"));
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()?;
    let fetched = async {
        let mut resp = http.get(MODEL_URL).send().await?.error_for_status()?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = tokio::fs::File::create(&archive).await?;
        let mut done = 0u64;
        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            on(ModelState::Downloading, done, total);
        }
        file.flush().await?;
        Ok::<_, Error>(())
    }
    .await;
    if let Err(e) = fetched {
        let _ = fs::remove_file(&archive);
        return Err(e);
    }

    on(ModelState::Unpacking, 0, 0);
    let unpacked = root.join(".unpacking");
    let _ = fs::remove_dir_all(&unpacked);
    fs::create_dir_all(&unpacked)?;
    let (a, u) = (archive.clone(), unpacked.clone());
    let untarred = tauri::async_runtime::spawn_blocking(move || untar(&a, &u))
        .await
        .map_err(|e| Error::msg(e.to_string()))
        .and_then(|r| r);
    let _ = fs::remove_file(&archive);
    let placed = untarred.and_then(|()| {
        let from = unpacked.join(MODEL);
        if !installed(&from) {
            return Err(Error::msg("The voice download is missing files"));
        }
        let to = root.join(MODEL);
        let _ = fs::remove_dir_all(&to);
        fs::rename(&from, &to)?;
        Ok(())
    });
    let _ = fs::remove_dir_all(&unpacked);
    placed
}

/// Windows' own tar (libarchive) reads .tar.bz2.
fn untar(archive: &Path, dest: &Path) -> Result<()> {
    let tar = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("tar.exe"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| "tar".into());
    let mut cmd = std::process::Command::new(tar);
    cmd.arg("-xf").arg(archive).arg("-C").arg(dest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if cmd.status()?.success() {
        Ok(())
    } else {
        Err(Error::msg("Couldn't unpack the voice download"))
    }
}

/// The loaded model, loading it first if needed.
fn engine(st: &AppState) -> Result<Arc<Mutex<OfflineTts>>> {
    let mut slot = st.assistant.speaker.engine.lock().unwrap();
    if let Some(e) = slot.as_ref() {
        return Ok(e.clone());
    }
    let dir = model_dir(&st.data_dir);
    if !installed(&dir) {
        return Err(Error::msg("The voice is still downloading"));
    }
    let started = Instant::now();
    let tts = load(&dir)?;
    let (sid, lang) = pick(&st.config.read().unwrap().settings, None);
    warm_up(&tts, sid, lang);
    eprintln!("voice: Kokoro loaded and warmed up in {:?}", started.elapsed());
    let e = Arc::new(Mutex::new(tts));
    *slot = Some(e.clone());
    Ok(e)
}

/// The first words after loading set up espeak-ng for the language, which added ~1.6 s to the
/// first sentence of the first reply; a word said to nobody does it beforehand.
fn warm_up(tts: &OfflineTts, sid: i32, lang: &str) {
    let _ = tts.generate_with_config("Ok.", &generation(sid, lang), None::<fn(&[f32], f32) -> bool>);
}

fn generation(sid: i32, lang: &str) -> GenerationConfig {
    GenerationConfig {
        sid,
        extra: Some(HashMap::from([("lang".to_string(), json!(lang))])),
        ..Default::default()
    }
}

fn load(dir: &Path) -> Result<OfflineTts> {
    let path = |f: &str| dir.join(f).to_string_lossy().into_owned();
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: Some(path("model.onnx")),
                voices: Some(path("voices.bin")),
                tokens: Some(path("tokens.txt")),
                data_dir: Some(path("espeak-ng-data")),
                dict_dir: Some(path("dict")),
                // English and Chinese words come from these; the rest from espeak-ng.
                lexicon: Some(format!("{},{}", path("lexicon-us-en.txt"), path("lexicon-zh.txt"))),
                ..Default::default()
            },
            num_threads: threads(),
            provider: Some("cpu".into()),
            ..Default::default()
        },
        // One sentence per callback: the first plays while the next is made.
        max_num_sentences: 1,
        silence_scale: 0.2,
        ..Default::default()
    };
    OfflineTts::create(&config).ok_or_else(|| Error::msg("Couldn't load the voice model"))
}

/// One per physical core (half the logical ones): 6 made the 8.8 s reply in 3.1 s, 4 in 3.4 s.
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, 6) as i32
}

/// The Kokoro voice for the launcher's language ("pt-BR").
fn language_voice(language: &str) -> &'static str {
    let lang = language.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    match lang.as_str() {
        "pt" => "pf_dora",
        "es" => "ef_dora",
        "fr" => "ff_siwis",
        "it" => "if_sara",
        "ja" => "jf_alpha",
        "zh" => "zf_xiaobei",
        "hi" => "hf_alpha",
        _ => "af_heart",
    }
}

/// A voice's speaker id and the espeak-ng language its text is read in: Kokoro's voice names
/// start with their language.
fn speaker(name: &str) -> Option<(i32, &'static str)> {
    let sid = VOICES.iter().position(|v| *v == name)?;
    let lang = match name.as_bytes()[0] {
        b'b' => "en-gb",
        b'e' => "es",
        b'f' => "fr-fr",
        b'h' => "hi",
        b'i' => "it",
        b'j' => "ja",
        b'p' => "pt-br",
        // American English; Chinese text goes through the Chinese lexicon whatever this says.
        _ => "en-us",
    };
    Some((sid as i32, lang))
}

/// `voice`, else the one picked in Settings, else the language's own (a typo still speaks).
fn pick(s: &Settings, voice: Option<&str>) -> (i32, &'static str) {
    voice
        .and_then(|v| speaker(v.trim()))
        .or_else(|| speaker(s.assistant_voice.trim()))
        .or_else(|| speaker(language_voice(&s.language)))
        .unwrap_or((3, "en-us"))
}

/// A line for trying a voice out, in its language.
pub fn sample(s: &Settings, voice: Option<&str>) -> &'static str {
    match pick(s, voice).1 {
        "pt-br" => "Oi! É assim que eu vou responder aqui na TV.",
        "es" => "¡Hola! Así es como voy a responder en la tele.",
        _ => "Hi! This is how I'll sound on the TV.",
    }
}

/// Drops markdown marks a model may still use; they'd be read out or trip the voice.
pub fn speakable(s: &str) -> String {
    let plain: String = s.chars().filter(|c| !matches!(c, '*' | '#' | '`' | '_' | '~')).collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reads `text` out on the speakers in `voice` (None: Settings' voice); returns once it has been
/// said or `hush`ed. "voice-start" (with `id`) goes out when the sound starts.
pub async fn say(app: &AppHandle, text: &str, voice: Option<&str>, id: u32) -> Result<()> {
    let st = app.state::<AppState>();
    let text = speakable(text);
    if text.is_empty() {
        return Ok(());
    }
    let (sid, lang) = pick(&st.config.read().unwrap().settings, voice);
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(earlier) = st.assistant.speaker.current.lock().unwrap().replace(stop.clone()) {
        earlier.store(true, Ordering::Relaxed);
    }
    let (a, s) = (app.clone(), stop.clone());
    let result = tauri::async_runtime::spawn_blocking(move || read_out(&a, &text, sid, lang, id, &s))
        .await
        .map_err(|e| Error::msg(e.to_string()))
        .and_then(|r| r);
    {
        let mut current = st.assistant.speaker.current.lock().unwrap();
        if current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &stop)) {
            *current = None;
        }
    }
    keep_warm(app);
    result
}

/// Stops the reply being read out.
pub fn hush(st: &AppState) {
    if let Some(stop) = st.assistant.speaker.current.lock().unwrap().take() {
        stop.store(true, Ordering::Relaxed);
    }
}

fn read_out(app: &AppHandle, text: &str, sid: i32, lang: &str, id: u32, stop: &Arc<AtomicBool>) -> Result<()> {
    let started = Instant::now();
    // Opens the speakers while the first sentence is made.
    let player = Player::start(stop.clone());
    let engine = engine(&app.state::<AppState>())?;
    let app = app.clone();
    play_out(player, &engine, text, sid, lang, stop, move || {
        eprintln!("voice: speaking {:?} after the reply came", started.elapsed());
        let _ = app.emit("voice-start", id);
    })
}

/// Makes `text` sentence by sentence into `player` and lets it play out. `started` runs once the
/// first sentence is on its way to the speakers.
fn play_out(
    mut player: Player,
    engine: &Mutex<OfflineTts>,
    text: &str,
    sid: i32,
    lang: &str,
    stop: &Arc<AtomicBool>,
    started: impl FnOnce() + Send + 'static,
) -> Result<()> {
    let tts = engine.lock().unwrap();
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    player.queue.lock().unwrap().voice_rate = tts.sample_rate() as u32;
    let (queue, halt) = (player.queue.clone(), stop.clone());
    let mut started = Some(started);
    let made = tts.generate_with_config(
        text,
        &generation(sid, lang),
        Some(move |samples: &[f32], _progress: f32| {
            queue.lock().unwrap().samples.extend(samples);
            if let Some(f) = started.take() {
                f();
            }
            !halt.load(Ordering::Relaxed)
        }),
    );
    // Free for the next reply while this one plays out.
    drop(tts);
    if made.is_none() && !stop.load(Ordering::Relaxed) {
        return Err(Error::msg("The voice couldn't read that"));
    }
    player.wait()
}

/// What the speakers play: the voice's samples at its own rate, stepped through at the device's.
#[derive(Default)]
struct Queue {
    samples: VecDeque<f32>,
    /// Position between samples[0] and samples[1].
    pos: f64,
    /// Both 0 until known.
    voice_rate: u32,
    device_rate: u32,
    /// Nothing more is coming.
    finished: bool,
}

impl Queue {
    /// The next sample at the device's rate (linear interpolation; fine for a voice). None while
    /// waiting for more.
    fn next(&mut self) -> Option<f32> {
        if self.voice_rate == 0 || self.device_rate == 0 || self.samples.len() < 2 {
            return None;
        }
        let a = self.samples[0];
        let b = self.samples[1];
        let v = a + (b - a) * self.pos as f32;
        self.pos += f64::from(self.voice_rate) / f64::from(self.device_rate);
        let whole = (self.pos as usize).min(self.samples.len() - 1);
        self.samples.drain(..whole);
        self.pos -= whole as f64;
        Some(v)
    }
}

/// The speakers, fed sentence by sentence. Its thread owns the audio stream (cpal's can't move
/// between threads) and closes it when the queue has played out, on `stop`, or when the
/// Player is dropped without `wait`.
struct Player {
    queue: Arc<Mutex<Queue>>,
    thread: Option<std::thread::JoinHandle<std::result::Result<(), String>>>,
}

impl Player {
    fn start(stop: Arc<AtomicBool>) -> Self {
        let queue: Arc<Mutex<Queue>> = Arc::default();
        let q = queue.clone();
        let thread = std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || play(&q, &stop))
            .expect("the voice's thread");
        Self { queue, thread: Some(thread) }
    }

    /// Lets the queue play out, then closes the speakers.
    fn wait(&mut self) -> Result<()> {
        {
            let mut q = self.queue.lock().unwrap();
            let tail = f64::from(q.voice_rate) * TAIL.as_secs_f64();
            q.samples.extend(std::iter::repeat_n(0.0, tail as usize));
            q.finished = true;
        }
        let Some(thread) = self.thread.take() else {
            return Ok(());
        };
        thread
            .join()
            .map_err(|_| Error::msg("The voice stopped unexpectedly"))?
            .map_err(|e| Error::msg(format!("Speakers: {e}")))
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let mut q = self.queue.lock().unwrap();
            q.samples.clear();
            q.finished = true;
        }
    }
}

fn play(queue: &Arc<Mutex<Queue>>, stop: &AtomicBool) -> std::result::Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no speakers found")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    queue.lock().unwrap().device_rate = config.sample_rate.0;
    let q = queue.clone();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, &config, q),
        SampleFormat::I16 => stream::<i16>(&device, &config, q),
        SampleFormat::U16 => stream::<u16>(&device, &config, q),
        SampleFormat::I32 => stream::<i32>(&device, &config, q),
        f => Err(format!("unsupported sample format {f}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;

    let mut left = usize::MAX;
    let mut moved = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        let q = queue.lock().unwrap();
        if q.finished && q.samples.len() < 2 {
            break;
        }
        if q.samples.len() != left || q.samples.len() < 2 {
            left = q.samples.len();
            moved = Instant::now();
        } else if moved.elapsed() > STALLED {
            return Err("the speakers stopped playing".into());
        }
    }
    Ok(())
}

fn stream<T>(device: &cpal::Device, config: &StreamConfig, queue: Arc<Mutex<Queue>>) -> std::result::Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut q = queue.lock().unwrap();
                for frame in data.chunks_mut(channels) {
                    frame.fill(T::from_sample(q.next().unwrap_or(0.0)));
                }
            },
            |e| eprintln!("voice: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_follows_language_unless_picked() {
        let mut s = Settings::default();
        assert_eq!(pick(&s, None), (42, "pt-br"));
        s.language = "en-US".into();
        assert_eq!(pick(&s, None), (3, "en-us"));
        s.assistant_voice = " pm_alex ".into();
        assert_eq!(pick(&s, None), (43, "pt-br"));
        // Trying another voice out; a misspelt one falls back to the language's.
        assert_eq!(pick(&s, Some("bm_george")), (26, "en-gb"));
        s.assistant_voice = "pf_dorra".into();
        assert_eq!(pick(&s, None), (3, "en-us"));
        assert_eq!(pick(&s, Some("em_santa")), (53, "es"));
    }

    #[test]
    fn markdown_is_not_read_out() {
        assert_eq!(speakable("**Done.** The  lights\nare _off_."), "Done. The lights are off.");
    }

    #[test]
    fn queue_resamples_and_waits_for_more() {
        let mut q = Queue { voice_rate: 24_000, device_rate: 48_000, ..Default::default() };
        assert_eq!(q.next(), None);
        q.samples.extend([0.0, 1.0, 0.0]);
        let out: Vec<f32> = std::iter::from_fn(|| q.next()).collect();
        assert_eq!(out, [0.0, 0.5, 1.0, 0.5]);
        // The last sample stays until the next one comes.
        assert_eq!(q.samples.len(), 1);
        q.samples.push_back(1.0);
        assert_eq!(q.next(), Some(0.0));
    }

    /// The model, if it's in this PC's data folder (or `KOKORO_DIR`): load time and how soon a
    /// typical reply's sentences are ready. `cargo test real_voice -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_voice() {
        let dir = std::env::var_os("KOKORO_DIR").map(PathBuf::from).unwrap_or_else(|| {
            model_dir(&PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("dev.tvlauncher.app"))
        });
        assert!(installed(&dir), "no model in {}", dir.display());
        let t = Instant::now();
        let tts = load(&dir).expect("loads");
        let (sid, lang) = speaker("pf_dora").unwrap();
        warm_up(&tts, sid, lang);
        println!("loaded and warmed up in {:?} with {} threads", t.elapsed(), threads());
        let text = "Pronto, liguei as luzes da sala. Amanhã faz 24 graus em Porto Alegre, com sol de manhã                     e chuva à tarde. Quer que eu coloque um alarme para as sete?";
        for (voice, text) in [("pf_dora", text), ("af_heart", "Done! The lights are on.")] {
            let (sid, lang) = speaker(voice).unwrap();
            let t = Instant::now();
            let ready: Arc<Mutex<Vec<Duration>>> = Arc::default();
            let r = ready.clone();
            let audio = tts
                .generate_with_config(text, &generation(sid, lang), Some(move |_: &[f32], _: f32| {
                    r.lock().unwrap().push(t.elapsed());
                    true
                }))
                .expect("speaks");
            let secs = audio.samples().len() as f32 / audio.sample_rate() as f32;
            let ready = ready.lock().unwrap();
            println!("{voice}: {secs:.1} s of audio in {:?}; sentences ready at {ready:?}", t.elapsed());
            assert!(secs > 1.0);
            // Each sentence is ready before the ones before it have played out.
            assert!(ready[0] < Duration::from_millis(1500), "first sentence took {:?}", ready[0]);
            let wav = std::env::temp_dir().join(format!("kokoro-{voice}.wav"));
            audio.save(&wav.to_string_lossy());
            println!("  saved {}", wav.display());
        }
    }

    /// Says two lines on the default speakers, the second cut short:
    /// `cargo test real_speakers -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_speakers() {
        let dir = model_dir(&PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("dev.tvlauncher.app"));
        let engine = Mutex::new(load(&dir).expect("loads"));
        let (sid, lang) = speaker("pf_dora").unwrap();
        warm_up(&engine.lock().unwrap(), sid, lang);

        let stop = Arc::new(AtomicBool::new(false));
        let t = Instant::now();
        let heard = Arc::new(Mutex::new(None));
        let h = heard.clone();
        play_out(Player::start(stop.clone()), &engine, "Teste do alto-falante. Um, dois, três.", sid, lang, &stop, move || {
            *h.lock().unwrap() = Some(t.elapsed());
        })
        .unwrap();
        let first = heard.lock().unwrap().expect("started");
        println!("first sound queued at {first:?}, finished after {:?}", t.elapsed());
        // It waits for the sound to play out (the line is ~2.5 s), not just for it to be made.
        assert!(t.elapsed() > Duration::from_secs(2), "returned after {:?}", t.elapsed());

        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            s.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let long = "Esta frase é bem comprida e vai ser interrompida no meio. Esta outra nem deveria tocar. \
                    E muito menos esta aqui, que é a última.";
        play_out(Player::start(stop.clone()), &engine, long, sid, lang, &stop, || {}).unwrap();
        println!("stopped after {:?}", t.elapsed());
        assert!(t.elapsed() < Duration::from_millis(3000), "hush took {:?}", t.elapsed());
    }

    /// Downloads and unpacks the real model into a temporary folder: `cargo test real_download --
    /// --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_download() {
        let root = std::env::temp_dir().join("tvl-voice-test");
        let _ = fs::remove_dir_all(&root);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let mut stages = Vec::new();
        rt.block_on(download(&root, |s, _, _| {
            if stages.last() != Some(&s) {
                stages.push(s);
            }
        }))
        .unwrap();
        assert_eq!(stages, [ModelState::Downloading, ModelState::Unpacking]);
        assert!(installed(&root.join(MODEL)));
        assert!(!root.join(".unpacking").exists());
        println!("model in {}", root.join(MODEL).display());
    }
}
