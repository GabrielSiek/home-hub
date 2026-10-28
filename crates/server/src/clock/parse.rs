//! Timers and alarms straight from what was said, before the model is asked: "timer de 12
//! minutos pra pizza", "wake me up at 7 on weekdays", "mais 5 minutos na pizza", "cancel all
//! timers". Portuguese and English. Only requests that are clearly one of these come out;
//! anything else, or anything half understood, is left to the model, which has the same
//! actions as tools.
//!
//! A request can hold several ("set a timer for 12 minutes for the pizza and wake me up at 7
//! tomorrow"): it's cut at each "and" / "e" / comma that isn't inside a length, a time or a list
//! of days, and every part has to be understood.

use tvl_kit::time::{HOUR, MIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Pt,
    En,
}

/// The day of a one-off alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Today,
    Tomorrow,
    /// The next Monday (0) … Sunday (6).
    Weekday(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Timer,
    Alarm,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Timer {
        ms: i64,
        label: Option<String>,
    },
    Alarm {
        hour: u8,
        minute: u8,
        /// Monday 0 … Sunday 6; none: once.
        days: Vec<u8>,
        when: Option<When>,
        label: Option<String>,
    },
    /// Cancels timers or turns alarms off: the one named, the one at `time`, or all of them.
    Cancel {
        target: Target,
        label: Option<String>,
        time: Option<(u8, u8)>,
        all: bool,
    },
    Add {
        ms: i64,
        label: Option<String>,
    },
    Pause {
        label: Option<String>,
    },
    Resume {
        label: Option<String>,
    },
    /// How long a timer has left.
    Left {
        label: Option<String>,
    },
}

#[derive(Debug)]
pub struct Parsed {
    pub lang: Lang,
    pub intents: Vec<Intent>,
}

pub fn parse(text: &str, default: Lang) -> Option<Parsed> {
    let words = words(text);
    let t: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    if t.is_empty() || t.len() > 48 {
        return None;
    }
    let mut intents = clauses(&t).into_iter().map(clause).collect::<Option<Vec<_>>>()?;
    // Names are matched without accents but shown as said: "Chá", not "Cha".
    let said: std::collections::HashMap<&str, &str> = words.iter().map(|(w, raw)| (w.as_str(), raw.as_str())).collect();
    let respell = |label: &mut Option<String>| {
        if let Some(l) = label {
            let lower = l.to_lowercase();
            *l = capitalize(&lower.split(' ').map(|w| *said.get(w).unwrap_or(&w)).collect::<Vec<_>>().join(" "));
        }
    };
    for i in &mut intents {
        match i {
            Intent::Timer { label, .. }
            | Intent::Alarm { label, .. }
            | Intent::Cancel { label, .. }
            | Intent::Add { label, .. }
            | Intent::Pause { label }
            | Intent::Resume { label }
            | Intent::Left { label } => respell(label),
        }
    }
    Some(Parsed { lang: lang(&t, default), intents })
}

// ---------- Words ----------

const TIMER: &[&str] = &["timer", "timers", "temporizador", "temporizadores", "cronometro", "cronometros", "countdown"];
const ALARM: &[&str] = &["alarm", "alarms", "alarme", "alarmes", "despertador", "despertadores"];
const PLURAL: &[&str] = &["timers", "temporizadores", "cronometros", "alarms", "alarmes", "despertadores"];
const ALL: &[&str] = &["all", "every", "todos", "todas", "tudo"];
const CANCEL: &[&str] = &[
    "cancel", "delete", "remove", "stop", "clear", "disable", "off", "dismiss", "cancela", "cancelar", "cancele",
    "apaga", "apague", "apagar", "remova", "remover", "tira", "tire", "tirar", "desliga", "desligue", "desligar",
    "desativa", "desative", "desativar", "exclui", "exclua", "excluir", "pare", "zera", "zerar",
];
const ADD: &[&str] = &["add", "adiciona", "adicione", "adicionar", "acrescenta", "acrescente", "acrescentar", "soma", "some"];
const PAUSE: &[&str] = &["pause", "pausa", "pausar", "pausa"];
const RESUME: &[&str] = &[
    "resume", "unpause", "continue", "continua", "continuar", "retoma", "retome", "retomar", "despausa", "despausar",
];
const LEFT: &[&str] = &["left", "remaining", "falta", "faltam", "resta", "restam"];
/// Asking for a new timer rather than more time on one: "another timer", "mais um timer".
const NEW: &[&str] = &["a", "an", "um", "uma", "another", "outro", "outra", "new", "novo", "nova"];
const ARTICLES: &[&str] = &[
    "the", "a", "an", "my", "this", "that", "o", "os", "as", "meu", "minha", "meus", "minhas", "esse", "essa", "este",
    "esta",
];
/// "Of", between "timer" and its name.
const OF: &[&str] = &["of", "do", "da", "de", "dos", "das"];
/// Little words and the request's own words: a name stops at them.
const STOP: &[&str] = &[
    "for", "to", "in", "at", "on", "of", "by", "with", "and", "then", "please", "now", "until", "when", "is", "are",
    "be", "will", "done", "ready", "finish", "finished", "end", "ends", "time", "how", "much", "long", "set", "start",
    "add", "more", "extra", "minute", "de", "do", "da", "dos", "das", "em", "no", "na", "nos", "nas", "pra", "pro",
    "pras", "pros", "para", "por", "com", "e", "depois", "agora", "favor", "que", "quando", "ate", "quanto", "tempo",
    "acabar", "acaba", "terminar", "termina", "ficar", "fica", "pronto", "pronta", "prontos", "prontas", "ser",
    "estar", "mais", ",", "all", "every", "todos", "todas", "tudo", "today", "tomorrow", "hoje", "amanha", "half",
    "quarter", "meia", "quarto", "daqui",
];
const PT: &[&str] = &[
    "de", "pra", "pro", "para", "as", "um", "uma", "minutos", "minuto", "horas", "hora", "segundos", "segundo", "alarme",
    "despertador", "acorda", "acorde", "acordar", "cancela", "cancelar", "mais", "quanto", "falta", "faltam", "pausa",
    "continua", "retoma", "coloca", "coloque", "poe", "bota", "marca", "liga", "desliga", "temporizador",
    "cronometro", "daqui", "amanha", "hoje", "meia", "manha", "tarde", "noite", "no", "na", "do", "da", "o", "todos",
    "todo", "dia", "dias", "me", "por", "favor", "e",
];
const EN: &[&str] = &[
    "the", "for", "set", "minutes", "minute", "hours", "hour", "seconds", "second", "wake", "up", "alarm", "cancel",
    "add", "how", "left", "at", "in", "my", "please", "start", "stop", "tomorrow", "today", "every", "weekdays",
    "weekends", "on", "and", "an", "a", "to", "of", "is",
];

fn has(t: &[&str], words: &[&str]) -> bool {
    t.iter().any(|w| words.contains(w))
}

/// `phrase` appears in `t`, word for word.
fn has_seq(t: &[&str], phrase: &[&str]) -> bool {
    t.windows(phrase.len()).any(|w| w == phrase)
}

fn lang(t: &[&str], default: Lang) -> Lang {
    let pt = t.iter().filter(|w| PT.contains(w)).count();
    let en = t.iter().filter(|w| EN.contains(w)).count();
    match pt.cmp(&en) {
        std::cmp::Ordering::Greater => Lang::Pt,
        std::cmp::Ordering::Less => Lang::En,
        std::cmp::Ordering::Equal => default,
    }
}

/// "Wake me up", "me acorda".
fn wake(t: &[&str]) -> bool {
    const PHRASES: &[&[&str]] = &[
        &["wake", "me"],
        &["wake", "up"],
        &["me", "acorda"],
        &["me", "acorde"],
        &["me", "acordar"],
        &["acorda", "me"],
        &["acorde", "me"],
        &["me", "desperta"],
        &["me", "desperte"],
    ];
    PHRASES.iter().any(|p| has_seq(t, p))
}

/// "Remind me in 10 minutes", "me avisa daqui a 10 minutos".
fn remind(t: &[&str]) -> bool {
    const VERBS: &[&str] = &["remind", "avisa", "avise", "lembra", "lembre", "chama", "chame"];
    has(t, VERBS) && has(t, &["me"]) && has(t, &["in", "em", "daqui"])
}

/// Lowercase words without accents, each with how it was written (lowercase). Digits split from
/// letters ("12min", "7h30"); a ':' or '.' between digits stays ("7:30", "1.5").
fn words(text: &str) -> Vec<(String, String)> {
    let raw: Vec<char> = text.to_lowercase().chars().collect();
    let mut out = Vec::new();
    let mut cur = (String::new(), String::new());
    let numeric = |c: char| c.is_ascii_digit() || c == ':' || c == '.';
    fn flush(cur: &mut (String, String), out: &mut Vec<(String, String)>) {
        if !cur.0.is_empty() {
            out.push(std::mem::take(cur));
        }
    }
    for (i, &r) in raw.iter().enumerate() {
        let between_digits =
            i > 0 && raw[i - 1].is_ascii_digit() && raw.get(i + 1).is_some_and(|n| n.is_ascii_digit());
        if r.is_alphanumeric() || (matches!(r, ':' | '.' | ',') && between_digits) {
            let c = match fold(r) {
                ',' => '.',
                c => c,
            };
            if cur.0.chars().last().is_some_and(|l| numeric(l) != numeric(c)) {
                flush(&mut cur, &mut out);
            }
            cur.0.push(c);
            cur.1.push(r);
        } else {
            flush(&mut cur, &mut out);
            if matches!(r, ',' | ';') {
                out.push((",".into(), ",".into()));
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// The words of `text` as they're matched: lowercase, no accents.
pub fn plain(text: &str) -> Vec<String> {
    words(text).into_iter().map(|(w, _)| w).collect()
}

fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        c => c,
    }
}

// ---------- Numbers, lengths, times, days ----------

fn small_number(w: &str) -> Option<i64> {
    Some(match w {
        "zero" => 0,
        "one" | "um" | "uma" => 1,
        "two" | "dois" | "duas" => 2,
        "three" | "tres" => 3,
        "four" | "quatro" => 4,
        "five" | "cinco" => 5,
        "six" | "seis" => 6,
        "seven" | "sete" => 7,
        "eight" | "oito" => 8,
        "nine" | "nove" => 9,
        "ten" | "dez" => 10,
        "eleven" | "onze" => 11,
        "twelve" | "doze" => 12,
        "thirteen" | "treze" => 13,
        "fourteen" | "catorze" | "quatorze" => 14,
        "fifteen" | "quinze" => 15,
        "sixteen" | "dezesseis" | "dezasseis" => 16,
        "seventeen" | "dezessete" | "dezassete" => 17,
        "eighteen" | "dezoito" => 18,
        "nineteen" | "dezenove" | "dezanove" => 19,
        _ => return None,
    })
}

fn tens(w: &str) -> Option<i64> {
    Some(match w {
        "twenty" | "vinte" => 20,
        "thirty" | "trinta" => 30,
        "forty" | "quarenta" => 40,
        "fifty" | "cinquenta" => 50,
        "sixty" | "sessenta" => 60,
        "ninety" | "noventa" => 90,
        _ => return None,
    })
}

fn is_number(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_digit()) || small_number(w).is_some() || tens(w).is_some()
}

/// The number at `t[i]`, and where it ends: "12", "1.5", "twelve", "vinte e cinco",
/// "twenty five". "A" / "um" count only before a unit ("an hour", "um minuto").
fn number(t: &[&str], i: usize) -> Option<(f64, usize)> {
    let w = *t.get(i)?;
    if w.starts_with(|c: char| c.is_ascii_digit()) {
        return w.parse::<f64>().ok().map(|n| (n, i + 1));
    }
    if matches!(w, "a" | "an" | "um" | "uma") {
        return t.get(i + 1).and_then(|n| unit(n)).map(|_| (1.0, i + 1));
    }
    if let Some(n) = small_number(w) {
        return Some((n as f64, i + 1));
    }
    let n = tens(w)?;
    let ones = |j: usize| t.get(j).and_then(|x| small_number(x)).filter(|u| (1..=9).contains(u));
    if t.get(i + 1) == Some(&"e") {
        if let Some(u) = ones(i + 2) {
            return Some(((n + u) as f64, i + 3));
        }
    }
    match ones(i + 1) {
        Some(u) => Some(((n + u) as f64, i + 2)),
        None => Some((n as f64, i + 1)),
    }
}

/// A unit of length, in ms.
fn unit(w: &str) -> Option<i64> {
    Some(match w {
        "h" | "hr" | "hrs" | "hour" | "hours" | "hora" | "horas" => HOUR,
        "m" | "min" | "mins" | "minute" | "minutes" | "minuto" | "minutos" => MIN,
        "s" | "sec" | "secs" | "second" | "seconds" | "seg" | "segundo" | "segundos" => 1000,
        _ => return None,
    })
}

fn starts(t: &[&str], i: usize, phrase: &[&str]) -> bool {
    t.get(i..i + phrase.len()).is_some_and(|w| w == phrase)
}

/// A length starting at `t[i]`: "12 minutes", "1h30", "uma hora e meia", "2 minutes and 30
/// seconds", "half an hour". Its ms, and where it ends.
fn length_at(t: &[&str], i: usize) -> Option<(i64, usize)> {
    const SET: &[(&[&str], i64)] = &[
        (&["meia", "hora"], 30 * MIN),
        (&["half", "an", "hour"], 30 * MIN),
        (&["half", "a", "minute"], 30_000),
        (&["half", "hour"], 30 * MIN),
        (&["quarter", "of", "an", "hour"], 15 * MIN),
        (&["a", "quarter", "hour"], 15 * MIN),
        (&["um", "quarto", "de", "hora"], 15 * MIN),
    ];
    if let Some((p, ms)) = SET.iter().find(|(p, _)| starts(t, i, p)) {
        return Some((*ms, i + p.len()));
    }
    let mut j = i;
    let mut total = 0.0;
    let mut last: Option<i64> = None;
    while let Some((n, k)) = number(t, j) {
        match t.get(k).and_then(|w| unit(w)) {
            Some(u) => {
                total += n * u as f64;
                last = Some(u);
                j = k + 1;
            }
            // "1h30", "1 hora e 15": the next unit down.
            None => match last {
                Some(u) if u > 1000 => {
                    total += n * (u / 60) as f64;
                    j = k;
                    last = Some(u / 60);
                }
                _ => break,
            },
        }
        let u = last.unwrap_or(MIN) as f64;
        if starts(t, j, &["e", "meia"]) {
            total += u / 2.0;
            j += 2;
        } else if starts(t, j, &["and", "a", "half"]) {
            total += u / 2.0;
            j += 3;
        }
        if matches!(t.get(j), Some(&("e" | "and" | ","))) && number(t, j + 1).is_some() {
            j += 1;
        }
    }
    (last.is_some() && total >= 1000.0).then_some((total.round() as i64, j))
}

/// The first length in `t`.
fn length(t: &[&str]) -> Option<(i64, usize, usize)> {
    (0..t.len()).find_map(|i| length_at(t, i).map(|(ms, end)| (ms, i, end)))
}

/// A time of day at `t[i]`: "7", "7:30", "7h30", "19h", "7 am", "7 da noite", "sete e meia",
/// "meio-dia", "half past 7". Its hour and minute, where it ends, and whether it says it's a
/// time ("7:30", "7h", "7 am", "7 o'clock") rather than being a bare number.
fn time_at(t: &[&str], i: usize) -> Option<(u8, u8, usize, bool)> {
    let w = *t.get(i)?;
    if starts(t, i, &["meio", "dia"]) || w == "noon" {
        return Some((12, 0, i + if w == "noon" { 1 } else { 2 }, true));
    }
    if starts(t, i, &["meia", "noite"]) || w == "midnight" {
        return Some((0, 0, i + if w == "midnight" { 1 } else { 2 }, true));
    }
    for (p, m) in [(&["half", "past"][..], 30), (&["quarter", "past"][..], 15)] {
        if starts(t, i, p) {
            let (h, _, end, _) = time_at(t, i + 2)?;
            return Some((h, m, end, true));
        }
    }
    let (mut h, mut m, mut j, mut marked) = match w.split_once(':') {
        Some((a, b)) => (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?, i + 1, true),
        None => {
            let (n, k) = number(t, i)?;
            if n.fract() != 0.0 {
                return None;
            }
            (n as u32, 0, k, false)
        }
    };
    if !marked {
        if matches!(t.get(j), Some(&("h" | "hr" | "hrs" | "horas" | "hora"))) {
            j += 1;
            marked = true;
            if let Some((mm, k)) = number(t, j).filter(|(mm, _)| *mm < 60.0 && mm.fract() == 0.0) {
                (m, j) = (mm as u32, k);
            }
        }
        if starts(t, j, &["e", "meia"]) {
            (m, j) = (30, j + 2);
        } else if t.get(j) == Some(&"e") {
            if let Some((mm, k)) = number(t, j + 1).filter(|(mm, _)| *mm < 60.0 && mm.fract() == 0.0) {
                (m, j) = (mm as u32, k);
                if matches!(t.get(j), Some(&("minutos" | "minuto" | "min"))) {
                    j += 1;
                }
            }
        }
        if starts(t, j, &["o", "clock"]) {
            (j, marked) = (j + 2, true);
        }
    }
    const AM: &[&[&str]] = &[&["am"], &["a", "m"], &["da", "manha"], &["de", "manha"], &["in", "the", "morning"], &["da", "madrugada"]];
    const PM: &[&[&str]] = &[
        &["pm"],
        &["p", "m"],
        &["da", "tarde"],
        &["de", "tarde"],
        &["a", "tarde"],
        &["da", "noite"],
        &["de", "noite"],
        &["a", "noite"],
        &["in", "the", "evening"],
        &["in", "the", "afternoon"],
        &["at", "night"],
        &["tonight"],
    ];
    if let Some(p) = AM.iter().find(|p| starts(t, j, p)) {
        (j, marked) = (j + p.len(), true);
        if h == 12 {
            h = 0;
        }
    } else if let Some(p) = PM.iter().find(|p| starts(t, j, p)) {
        (j, marked) = (j + p.len(), true);
        if h < 12 {
            h += 12;
        }
    }
    (h < 24 && m < 60).then_some((h as u8, m as u8, j, marked))
}

/// The alarm's time in `t`: after "at" / "às" / "para as"… or one that says it's a time, and
/// not a length ("in 10 minutes", "de 2 horas").
fn alarm_time(t: &[&str]) -> Option<(u8, u8)> {
    const AT: &[&str] = &["at", "as", "pras", "para", "pra", "for", "by", "das", "@"];
    const AT_HOURS: &[&str] = &["at", "as", "pras", "para", "pra", "das"];
    for i in 0..t.len() {
        let before = i.checked_sub(1).map(|j| t[j]);
        let daqui_a = before == Some("a") && i >= 2 && t[i - 2] == "daqui";
        if matches!(before, Some("in" | "em" | "daqui")) || daqui_a {
            continue;
        }
        let Some((h, m, end, marked)) = time_at(t, i) else { continue };
        // A length, unless it's "às 7 horas" ("7h" and "7h30" are times).
        if let Some(u) = t.get(i + 1).filter(|w| **w != "h").and_then(|w| unit(w)) {
            if u < HOUR || !before.is_some_and(|b| AT_HOURS.contains(&b)) {
                continue;
            }
        }
        if t.get(end).is_some_and(|w| unit(w).is_some() && !matches!(*w, "h" | "hora" | "horas")) {
            continue;
        }
        if marked || before.is_some_and(|b| AT.contains(&b)) {
            return Some((h, m));
        }
    }
    None
}

fn day_word(w: &str) -> Option<(u8, bool)> {
    Some(match w {
        "monday" | "mon" | "segunda" => (0, false),
        "mondays" | "segundas" => (0, true),
        "tuesday" | "tue" | "tues" | "terca" => (1, false),
        "tuesdays" | "tercas" => (1, true),
        "wednesday" | "wed" | "quarta" => (2, false),
        "wednesdays" | "quartas" => (2, true),
        "thursday" | "thu" | "thurs" | "quinta" => (3, false),
        "thursdays" | "quintas" => (3, true),
        "friday" | "fri" | "sexta" => (4, false),
        "fridays" | "sextas" => (4, true),
        "saturday" | "sat" | "sabado" => (5, false),
        "saturdays" | "sabados" => (5, true),
        "sunday" | "sun" | "domingo" => (6, false),
        "sundays" | "domingos" => (6, true),
        _ => return None,
    })
}

/// The days an alarm repeats on, and the day of a one-off one.
fn days(t: &[&str]) -> (Vec<u8>, Option<When>) {
    const EVERY_DAY: &[&[&str]] = &[
        &["every", "day"],
        &["everyday"],
        &["daily"],
        &["each", "day"],
        &["todo", "dia"],
        &["todos", "os", "dias"],
        &["diariamente"],
    ];
    const WEEKDAYS: &[&[&str]] = &[
        &["weekdays"],
        &["weekday"],
        &["workdays"],
        &["dias", "de", "semana"],
        &["dia", "de", "semana"],
        &["dias", "uteis"],
        &["dia", "util"],
        &["durante", "a", "semana"],
        &["segunda", "a", "sexta"],
        &["monday", "to", "friday"],
        &["monday", "through", "friday"],
    ];
    const WEEKENDS: &[&[&str]] = &[
        &["weekends"],
        &["weekend"],
        &["fim", "de", "semana"],
        &["fins", "de", "semana"],
        &["final", "de", "semana"],
        &["finais", "de", "semana"],
    ];
    let any = |list: &[&[&str]]| list.iter().any(|p| has_seq(t, p));
    let mut out: Vec<u8> = Vec::new();
    if any(EVERY_DAY) {
        out.extend(0..7);
    }
    if any(WEEKDAYS) {
        out.extend(0..5);
    }
    if any(WEEKENDS) {
        out.extend([5, 6]);
    }
    let mut once = None;
    if out.is_empty() {
        for (i, w) in t.iter().enumerate() {
            let Some((d, plural)) = day_word(w) else { continue };
            let every = t[i.saturating_sub(2)..i]
                .iter()
                .any(|b| matches!(*b, "every" | "each" | "toda" | "todas" | "todo" | "todos" | "as" | "nas" | "aos"));
            if plural || every {
                out.push(d);
            } else {
                once.get_or_insert(When::Weekday(d));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    if !out.is_empty() {
        return (out, None);
    }
    if has(t, &["tomorrow", "amanha"]) {
        once = Some(When::Tomorrow);
    } else if once.is_none() && has(t, &["today", "hoje", "tonight"]) {
        once = Some(When::Today);
    }
    (out, once)
}

// ---------- Names ----------

fn ends_name(w: &str) -> bool {
    STOP.contains(&w) || is_number(w) || unit(w).is_some() || TIMER.contains(&w) || ALARM.contains(&w)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// The name from `t[i]` on: leading articles, "timer" and "of" skipped, up to three words, until
/// a little word.
fn name_at(t: &[&str], i: usize) -> Option<String> {
    let mut i = i;
    while t.get(i).is_some_and(|w| ARTICLES.contains(w) || TIMER.contains(w) || OF.contains(w)) {
        i += 1;
    }
    let mut words: Vec<&str> = t.get(i..)?.iter().take_while(|w| !ends_name(w)).take(3).copied().collect();
    while words.last().is_some_and(|w| ARTICLES.contains(w)) {
        words.pop();
    }
    (!words.is_empty()).then(|| capitalize(&words.join(" ")))
}

/// A timer's name: "pizza timer", "timer da pizza", "for the pizza", "pra pizza".
fn timer_label(t: &[&str]) -> Option<String> {
    const START: &[&str] = &[
        "set", "start", "create", "make", "new", "another", "coloca", "coloque", "poe", "bota", "marca", "liga", "cria",
        "inicia", "comeca", "cancel", "pause", "resume", "stop",
    ];
    for (k, w) in t.iter().enumerate() {
        if !TIMER.contains(w) {
            continue;
        }
        // "pizza timer", "the pasta timer".
        let before: Vec<&str> = t[..k]
            .iter()
            .rev()
            .take_while(|b| !ends_name(b) && !ARTICLES.contains(b) && !START.contains(b) && !NEW.contains(b))
            .take(2)
            .copied()
            .collect();
        if !before.is_empty() {
            return Some(capitalize(&before.into_iter().rev().collect::<Vec<_>>().join(" ")));
        }
        // "timer da pizza".
        if t.get(k + 1).is_some_and(|n| OF.contains(n)) {
            if let Some(name) = name_at(t, k + 2) {
                return Some(name);
            }
        }
    }
    const FOR: &[&str] = &["for", "to", "pra", "pro", "para", "called", "named", "chamado", "chamada"];
    (0..t.len()).filter(|&i| FOR.contains(&t[i])).find_map(|i| name_at(t, i + 1))
}

/// What a request acts on: the name after the verb ("pausa o timer da pizza"), after "to" /
/// "no" / "pra"… ("mais 5 minutos na pizza"), or a timer's own name.
fn object_label(t: &[&str], verbs: &[&str]) -> Option<String> {
    const ON: &[&str] = &["to", "on", "for", "until", "no", "na", "nos", "nas", "pro", "pra", "para", "ao", "em", "in"];
    let after_verb = t.iter().position(|w| verbs.contains(w)).and_then(|v| name_at(t, v + 1));
    after_verb
        .or_else(|| (0..t.len()).filter(|&i| ON.contains(&t[i])).find_map(|i| name_at(t, i + 1)))
        .or_else(|| timer_label(t))
}

// ---------- Requests ----------

/// Cuts at "and" / "e" / commas, except inside a length, a time or a list of days ("1 hora e
/// 30", "7 e meia", "sábado e domingo", "vinte e cinco").
fn clauses<'a>(t: &'a [&'a str]) -> Vec<&'a [&'a str]> {
    let inner = |w: &str| is_number(w) || unit(w).is_some() || day_word(w).is_some();
    let mut out = Vec::new();
    let mut from = 0;
    for i in 0..t.len() {
        let cut = match t[i] {
            "," | "then" | "depois" | "also" | "tambem" => true,
            "and" | "e" => {
                let prev = i.checked_sub(1).map(|j| t[j]).unwrap_or("");
                let next = t.get(i + 1).copied().unwrap_or("");
                let half = matches!(next, "meia" | "half") || (next == "a" && t.get(i + 2) == Some(&"half"));
                !(inner(prev) && (inner(next) || half))
            }
            _ => false,
        };
        if cut {
            if i > from {
                out.push(&t[from..i]);
            }
            from = i + 1;
        }
    }
    if from < t.len() {
        out.push(&t[from..]);
    }
    out
}

/// `verb` comes no more than five words before one of `nouns`.
fn acts_on(t: &[&str], verbs: &[&str], nouns: &[&str]) -> bool {
    t.iter()
        .enumerate()
        .filter(|(_, w)| verbs.contains(w))
        .any(|(i, _)| t[i + 1..(i + 6).min(t.len())].iter().any(|w| nouns.contains(w)))
}

fn clause(t: &[&str]) -> Option<Intent> {
    let timer = has(t, TIMER);
    let alarm = has(t, ALARM) || wake(t);
    let new_timer = t.windows(2).any(|w| NEW.contains(&w[0]) && TIMER.contains(&w[1]));

    if acts_on(t, CANCEL, TIMER) || acts_on(t, CANCEL, ALARM) || has_seq(t, &["timer", "off"]) {
        let target = match (timer, has(t, ALARM)) {
            (true, false) => Target::Timer,
            (false, true) => Target::Alarm,
            _ => return None,
        };
        let all = has(t, ALL) || has(t, PLURAL);
        return Some(match target {
            Target::Timer => Intent::Cancel { target, label: object_label(t, CANCEL), time: None, all },
            Target::Alarm => Intent::Cancel { target, label: None, time: alarm_time(t), all },
        });
    }

    let more = t.windows(2).any(|w| {
        (matches!(w[0], "mais" | "another") && is_number(w[1])) || (is_number(w[0]) && matches!(w[1], "more" | "extra"))
    });
    if (has(t, ADD) || more) && !new_timer && !alarm {
        let without: Vec<&str> = t.iter().copied().filter(|w| !matches!(*w, "more" | "extra")).collect();
        let (ms, _, _) = length(&without)?;
        return Some(Intent::Add { ms, label: object_label(t, ADD) });
    }

    if has(t, PAUSE) || has(t, RESUME) {
        let verbs = if has(t, PAUSE) { PAUSE } else { RESUME };
        let label = object_label(t, verbs);
        if !timer && label.is_none() {
            return None;
        }
        return Some(if verbs == PAUSE { Intent::Pause { label } } else { Intent::Resume { label } });
    }

    let asks = has_seq(t, &["how", "much", "time"])
        || has_seq(t, &["how", "long"])
        || has_seq(t, &["quanto", "tempo"])
        || has_seq(t, &["quanto", "falta"])
        || has_seq(t, &["quanto", "resta"])
        || has_seq(t, &["falta", "quanto"])
        || has_seq(t, &["faltam", "quanto"]);
    if asks && !alarm {
        let label = object_label(t, &[]);
        if !timer && label.is_none() && !has(t, LEFT) {
            return None;
        }
        return Some(Intent::Left { label });
    }

    // "Wake me up when the timer ends": neither.
    if alarm && timer {
        return None;
    }
    if alarm {
        if let Some((hour, minute)) = alarm_time(t) {
            let (days, when) = days(t);
            let label = name_after(t, &["called", "named", "chamado", "nome"]).or_else(|| wake(t).then(|| "Wake up".into()));
            return Some(Intent::Alarm { hour, minute, days, when, label });
        }
        // "Um alarme de 10 minutos": a timer.
        let (ms, _, _) = length(t)?;
        return Some(Intent::Timer { ms, label: None });
    }

    if timer || remind(t) {
        let (ms, _, _) = length(t)?;
        return Some(Intent::Timer { ms, label: timer_label(t) });
    }
    None
}

fn name_after(t: &[&str], markers: &[&str]) -> Option<String> {
    (0..t.len()).filter(|&i| markers.contains(&t[i])).find_map(|i| name_at(t, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Intent {
        let p = parse(text, Lang::En).unwrap_or_else(|| panic!("not understood: {text}"));
        assert_eq!(p.intents.len(), 1, "{text}: {:?}", p.intents);
        p.intents.into_iter().next().unwrap()
    }

    fn timer(ms: i64, label: Option<&str>) -> Intent {
        Intent::Timer { ms, label: label.map(str::to_string) }
    }

    fn alarm(hour: u8, minute: u8, days: &[u8], when: Option<When>, label: Option<&str>) -> Intent {
        Intent::Alarm { hour, minute, days: days.to_vec(), when, label: label.map(str::to_string) }
    }

    #[test]
    fn words() {
        assert_eq!(plain("Às 7h30, cronômetro de 12min!"), ["as", "7", "h", "30", ",", "cronometro", "de", "12", "min"]);
        assert_eq!(plain("wake me at 6:45 am"), ["wake", "me", "at", "6:45", "am"]);
        assert_eq!(plain("1,5 hora"), ["1.5", "hora"]);
    }

    #[test]
    fn timers() {
        assert_eq!(one("Set a timer for 12 minutes for the pizza"), timer(12 * MIN, Some("Pizza")));
        assert_eq!(one("timer de 12 minutos pra pizza"), timer(12 * MIN, Some("Pizza")));
        assert_eq!(one("coloca um timer de 5 minutos"), timer(5 * MIN, None));
        assert_eq!(one("set a pizza timer for 12 minutes"), timer(12 * MIN, Some("Pizza")));
        assert_eq!(one("start a 10-minute timer"), timer(10 * MIN, None));
        assert_eq!(one("timer do bolo de 40 minutos"), timer(40 * MIN, Some("Bolo")));
        assert_eq!(one("temporizador de uma hora e meia"), timer(90 * MIN, None));
        assert_eq!(one("timer de 1h30"), timer(90 * MIN, None));
        assert_eq!(one("timer de 1 hora e 15 minutos"), timer(75 * MIN, None));
        assert_eq!(one("timer for 2 minutes and 30 seconds"), timer(150_000, None));
        assert_eq!(one("timer de vinte e cinco minutos para o arroz"), timer(25 * MIN, Some("Arroz")));
        assert_eq!(one("set a timer for half an hour"), timer(30 * MIN, None));
        assert_eq!(one("timer de meia hora"), timer(30 * MIN, None));
        assert_eq!(one("me avisa daqui a 10 minutos"), timer(10 * MIN, None));
        assert_eq!(one("remind me in 15 minutes to check the oven"), timer(15 * MIN, Some("Check the oven")));
        assert_eq!(one("põe um alarme de 10 minutos"), timer(10 * MIN, None));
        assert_eq!(one("can you set a timer for 90 seconds please"), timer(90_000, None));
    }

    #[test]
    fn alarms() {
        assert_eq!(one("wake me up at 7"), alarm(7, 0, &[], None, Some("Wake up")));
        assert_eq!(one("me acorda amanhã às 6:30"), alarm(6, 30, &[], Some(When::Tomorrow), Some("Wake up")));
        assert_eq!(one("alarme às 7 nos dias de semana"), alarm(7, 0, &[0, 1, 2, 3, 4], None, None));
        assert_eq!(one("set an alarm for 6:15 every day"), alarm(6, 15, &[0, 1, 2, 3, 4, 5, 6], None, None));
        assert_eq!(one("despertador para as 9 no fim de semana"), alarm(9, 0, &[5, 6], None, None));
        assert_eq!(one("alarme 7h30 às segundas e quartas"), alarm(7, 30, &[0, 2], None, None));
        assert_eq!(one("alarm at 7 pm on friday"), alarm(19, 0, &[], Some(When::Weekday(4)), None));
        assert_eq!(one("me acorda às 7 e meia"), alarm(7, 30, &[], None, Some("Wake up")));
        assert_eq!(one("alarme às sete da noite"), alarm(19, 0, &[], None, None));
        assert_eq!(one("alarme ao meio-dia"), alarm(12, 0, &[], None, None));
        assert_eq!(one("wake me up at 7 o'clock tomorrow"), alarm(7, 0, &[], Some(When::Tomorrow), Some("Wake up")));
    }

    #[test]
    fn changes() {
        assert_eq!(
            one("cancel the pizza timer"),
            Intent::Cancel { target: Target::Timer, label: Some("Pizza".into()), time: None, all: false }
        );
        assert_eq!(
            one("cancela todos os timers"),
            Intent::Cancel { target: Target::Timer, label: None, time: None, all: true }
        );
        assert_eq!(
            one("desliga o alarme das 7"),
            Intent::Cancel { target: Target::Alarm, label: None, time: Some((7, 0)), all: false }
        );
        assert_eq!(one("add 5 minutes to the pizza"), Intent::Add { ms: 5 * MIN, label: Some("Pizza".into()) });
        assert_eq!(one("mais 5 minutos no timer da pizza"), Intent::Add { ms: 5 * MIN, label: Some("Pizza".into()) });
        assert_eq!(one("adiciona 2 minutos"), Intent::Add { ms: 2 * MIN, label: None });
        assert_eq!(one("5 more minutes on the tea"), Intent::Add { ms: 5 * MIN, label: Some("Tea".into()) });
        assert_eq!(one("pausa o timer da pizza"), Intent::Pause { label: Some("Pizza".into()) });
        assert_eq!(one("resume the timer"), Intent::Resume { label: None });
        assert_eq!(one("quanto tempo falta pra pizza ficar pronta"), Intent::Left { label: Some("Pizza".into()) });
        assert_eq!(one("how much time is left"), Intent::Left { label: None });
    }

    #[test]
    fn several_at_once() {
        let p = parse("Set a timer for 12 minutes for the pizza and wake me up at 7 tomorrow", Lang::Pt).unwrap();
        assert_eq!(p.lang, Lang::En);
        assert_eq!(
            p.intents,
            [timer(12 * MIN, Some("Pizza")), alarm(7, 0, &[], Some(When::Tomorrow), Some("Wake up"))]
        );
        let p = parse("timer de 10 minutos pro chá e alarme às 6 de segunda a sexta", Lang::En).unwrap();
        assert_eq!(p.lang, Lang::Pt);
        assert_eq!(p.intents, [timer(10 * MIN, Some("Chá")), alarm(6, 0, &[0, 1, 2, 3, 4], None, None)]);
        let p = parse("alarm at 7 and a timer for 5 minutes", Lang::Pt).unwrap();
        assert_eq!(p.intents, [alarm(7, 0, &[], None, None), timer(5 * MIN, None)]);
    }

    #[test]
    fn leaves_the_rest_to_the_model() {
        for text in [
            "what's the weather tomorrow",
            "set an alarm",
            "set a timer",
            "turn off the lights and set a timer for 10 minutes",
            "desliga a luz e coloca um timer de 10 minutos",
            "wake me up when the timer ends",
            "pause",
            "how long is the movie",
            "what time is my alarm",
            "add the movie to my list",
            "open netflix",
        ] {
            assert!(parse(text, Lang::En).is_none(), "{text}");
        }
    }
}
