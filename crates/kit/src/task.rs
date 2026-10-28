//! Background loops that don't die quietly. In the old launcher a panic inside a loop (the clock's
//! tick, the automations) ended it for good with nothing in a log; here it's logged and the loop
//! starts again, and every loop stops when the program is shutting down.

use std::any::Any;
use std::future::Future;
use std::time::Duration;

pub use tokio_util::sync::CancellationToken;

/// Runs the loop `make()` returns until `shutdown` is cancelled. A panic is logged and the loop is
/// made again after a pause that grows from 1 s to a minute. A loop that returns is done.
pub fn spawn_supervised<F, Fut>(name: &'static str, shutdown: CancellationToken, make: F) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    supervise(name, shutdown, Duration::from_secs(1), make)
}

fn supervise<F, Fut>(name: &'static str, shutdown: CancellationToken, first_pause: Duration, make: F) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut pause = first_pause;
        loop {
            let run = tokio::spawn(make());
            let abort = run.abort_handle();
            tokio::select! {
                _ = shutdown.cancelled() => {
                    abort.abort();
                    return;
                }
                done = run => match done {
                    Ok(()) => return,
                    Err(e) if e.is_panic() => {
                        tracing::error!(task = name, "crashed, starting it again in {pause:?}: {}", panic_text(e.into_panic()));
                    }
                    Err(_) => return,
                },
            }
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(pause) => {}
            }
            pause = (pause * 2).min(Duration::from_secs(60));
        }
    })
}

/// Runs `fut` once in the background; an error is logged instead of dropped.
pub fn spawn_logged<Fut, E>(name: &'static str, fut: Fut) -> tokio::task::JoinHandle<()>
where
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: std::fmt::Display,
{
    tokio::spawn(async move {
        if let Err(e) = fut.await {
            tracing::warn!(task = name, "{e}");
        }
    })
}

fn panic_text(p: Box<dyn Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn a_crashed_loop_starts_again() {
        let runs = Arc::new(AtomicUsize::new(0));
        let counted = runs.clone();
        let task = supervise("test", CancellationToken::new(), Duration::from_millis(5), move || {
            let runs = counted.clone();
            async move {
                if runs.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("first run fails");
                }
            }
        });
        tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn shutting_down_stops_the_loop() {
        let shutdown = CancellationToken::new();
        let task = spawn_supervised("forever", shutdown.clone(), || async {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
    }
}
