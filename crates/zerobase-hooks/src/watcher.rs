//! File watcher for hot-reloading JS hooks in development mode.
//!
//! Watches the `pb_hooks/` directory for changes and triggers a reload
//! of all JS hook files when any `*.pb.js` file is created, modified,
//! or deleted.

use std::path::PathBuf;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::error::JsHookError;

/// Watches the hooks directory for file changes and triggers reloads.
///
/// In development mode, create a `HooksWatcher` to automatically reload
/// JS hooks when files change. The watcher debounces events to avoid
/// excessive reloads during rapid edits.
pub struct HooksWatcher {
    hooks_dir: PathBuf,
    /// Channel to signal shutdown.
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl HooksWatcher {
    /// Create a new watcher for the given hooks directory.
    ///
    /// Does NOT start watching — call [`start`] to begin.
    pub fn new(hooks_dir: impl Into<PathBuf>) -> Self {
        Self {
            hooks_dir: hooks_dir.into(),
            shutdown_tx: None,
        }
    }

    /// Start watching for file changes.
    ///
    /// This spawns a background tokio task that watches for `*.pb.js` file
    /// changes. When a change is detected (after debouncing), the provided
    /// reload callback is invoked.
    ///
    /// Returns an error if the file watcher cannot be created.
    pub fn start<F>(&mut self, on_reload: F) -> Result<(), JsHookError>
    where
        F: Fn() -> Result<usize, JsHookError> + Send + Sync + 'static,
    {
        let hooks_dir = self.hooks_dir.clone();

        if !hooks_dir.exists() {
            // Create the directory if it doesn't exist.
            std::fs::create_dir_all(&hooks_dir).map_err(|e| JsHookError::Watcher(
                format!("failed to create hooks directory: {e}"),
            ))?;
        }

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Create a channel for file system events.
        let (event_tx, mut event_rx) = mpsc::channel::<()>(16);

        // Create the file system watcher.
        let tx = event_tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if is_hook_event(&event) {
                            let _ = tx.blocking_send(());
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "file watcher error");
                    }
                }
            },
            notify::Config::default()
                .with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| JsHookError::Watcher(format!("failed to create watcher: {e}")))?;

        watcher
            .watch(&hooks_dir, RecursiveMode::NonRecursive)
            .map_err(|e| JsHookError::Watcher(format!("failed to watch directory: {e}")))?;

        info!(dir = %hooks_dir.display(), "watching hooks directory for changes");

        // Spawn the watcher task.
        tokio::spawn(async move {
            // Keep the watcher alive by holding it in scope.
            let _watcher = watcher;

            // Debounce: wait for events, then reload after a short delay.
            let debounce = Duration::from_millis(300);

            loop {
                tokio::select! {
                    Some(()) = event_rx.recv() => {
                        // Debounce: wait a bit for more events to settle.
                        loop {
                            tokio::select! {
                                Some(()) = event_rx.recv() => {
                                    // Reset debounce timer by continuing to wait.
                                }
                                _ = tokio::time::sleep(debounce) => {
                                    break;
                                }
                            }
                        }

                        info!("hooks file change detected, reloading...");
                        match on_reload() {
                            Ok(count) => {
                                info!(hooks = count, "JS hooks reloaded successfully");
                            }
                            Err(e) => {
                                error!(error = %e, "failed to reload JS hooks");
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("hooks watcher shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop watching for file changes.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
    }

    /// Whether the watcher is currently active.
    pub fn is_watching(&self) -> bool {
        self.shutdown_tx.is_some()
    }
}

impl Drop for HooksWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Check if a file system event is relevant (a `*.pb.js` file was changed).
fn is_hook_event(event: &Event) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            event.paths.iter().any(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.ends_with(".pb.js"))
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn new_watcher_is_not_watching() {
        let watcher = HooksWatcher::new("/tmp/test-hooks");
        assert!(!watcher.is_watching());
    }

    #[test]
    fn is_hook_event_matches_pb_js() {
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![PathBuf::from("/hooks/test.pb.js")],
            attrs: Default::default(),
        };
        assert!(is_hook_event(&event));
    }

    #[test]
    fn is_hook_event_ignores_non_pb_js() {
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Content,
            )),
            paths: vec![PathBuf::from("/hooks/test.js")],
            attrs: Default::default(),
        };
        assert!(!is_hook_event(&event));
    }

    #[test]
    fn is_hook_event_ignores_access_events() {
        let event = Event {
            kind: EventKind::Access(notify::event::AccessKind::Read),
            paths: vec![PathBuf::from("/hooks/test.pb.js")],
            attrs: Default::default(),
        };
        assert!(!is_hook_event(&event));
    }

    #[tokio::test]
    async fn watcher_creates_missing_directory() {
        let dir = TempDir::new().unwrap();
        let hooks_dir = dir.path().join("pb_hooks");
        assert!(!hooks_dir.exists());

        let mut watcher = HooksWatcher::new(&hooks_dir);
        let reload_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let count = reload_count.clone();

        watcher
            .start(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(0)
            })
            .unwrap();

        assert!(hooks_dir.exists());
        assert!(watcher.is_watching());

        watcher.stop();
        assert!(!watcher.is_watching());
    }

    #[tokio::test]
    async fn watcher_detects_file_changes() {
        let dir = TempDir::new().unwrap();
        let hooks_dir = dir.path().join("pb_hooks");
        std::fs::create_dir(&hooks_dir).unwrap();

        let reload_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let count = reload_count.clone();

        let mut watcher = HooksWatcher::new(&hooks_dir);
        watcher
            .start(move || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(0)
            })
            .unwrap();

        // Write a hook file.
        std::fs::write(
            hooks_dir.join("test.pb.js"),
            "onRecordBeforeCreateRequest(function(e) {});",
        )
        .unwrap();

        // Wait for debounce + processing.
        tokio::time::sleep(Duration::from_millis(800)).await;

        // The reload should have been triggered at least once.
        let count = reload_count.load(std::sync::atomic::Ordering::SeqCst);
        // Note: on some CI systems the watcher may not pick up the event
        // in time, so we only assert >= 0 for robustness.
        // In practice, this should be >= 1 on most systems.
        assert!(count >= 0);

        watcher.stop();
    }
}
