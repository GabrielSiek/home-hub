//! Logging with `tracing`: to the console, and to a file a day in a folder. The old launcher's
//! release build had no console, so its 115 prints went nowhere.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

/// Starts logging: `info` and up unless `TVL_LOG` says otherwise (e.g. `TVL_LOG=debug` or
/// `TVL_LOG=tvl_server=debug,info`). With `dir`, also to `dir\<name>.<date>.log`, keeping two
/// weeks. Keep the returned guard alive until exit, or the last lines can be lost. A second call
/// does nothing.
pub fn init(dir: Option<&Path>, name: &str) -> Option<WorkerGuard> {
    let filter = || EnvFilter::try_from_env("TVL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let console = fmt::layer().with_writer(std::io::stderr).with_target(false).with_filter(filter());
    let (file, guard) = match dir.map(|d| file_writer(d, name)) {
        Some(Ok((writer, guard))) => {
            (Some(fmt::layer().with_writer(writer).with_ansi(false).with_filter(filter())), Some(guard))
        }
        Some(Err(e)) => {
            eprintln!("logging only to the console: {e}");
            (None, None)
        }
        None => (None, None),
    };
    let _ = tracing_subscriber::registry().with(console).with(file).try_init();
    guard
}

fn file_writer(
    dir: &Path,
    name: &str,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dir)?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(name)
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir)?;
    Ok(tracing_appender::non_blocking(appender))
}

/// A secret as it may appear in a log or on a screen: "••••3f9a", or "" when unset.
pub fn masked(secret: &str) -> String {
    let s = secret.trim();
    if s.is_empty() {
        return String::new();
    }
    let tail: String = s.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    if s.chars().count() <= 8 {
        "••••".into()
    } else {
        format!("••••{tail}")
    }
}

/// At most `n` characters of `s`, for log lines.
pub fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_show_only_their_last_characters() {
        assert_eq!(masked(""), "");
        assert_eq!(masked("short"), "••••");
        assert_eq!(masked("abcdefgh12345"), "••••2345");
    }

    #[test]
    fn long_text_is_clipped() {
        assert_eq!(clip("turn on the lights", 7), "turn on…");
        assert_eq!(clip("ok", 7), "ok");
    }
}
