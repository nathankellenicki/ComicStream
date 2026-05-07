// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use notify_debouncer_full::notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub fn spawn(library: PathBuf, debounce: Duration, tx: mpsc::Sender<()>) -> Result<()> {
    let (sync_tx, sync_rx) = std::sync::mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(debounce, None, move |res| {
        let _ = sync_tx.send(res);
    })
    .context("creating filesystem debouncer")?;

    debouncer
        .watcher()
        .watch(&library, RecursiveMode::Recursive)
        .with_context(|| format!("watching {}", library.display()))?;

    info!(
        library = %library.display(),
        debounce_ms = debounce.as_millis() as u64,
        "watcher started"
    );

    std::thread::spawn(move || {
        let _keep_alive = debouncer;
        for res in sync_rx {
            match res {
                Ok(events) if !events.is_empty() => {
                    debug!(events = events.len(), "watcher: triggering rescan");
                    match tx.try_send(()) {
                        Ok(_) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            debug!("watcher: rescan already pending");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            warn!("watcher: scan channel closed, exiting");
                            break;
                        }
                    }
                }
                Ok(_) => {}
                Err(errs) => {
                    for e in errs {
                        warn!("watcher error: {}", e);
                    }
                }
            }
        }
    });

    Ok(())
}
