// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info};

pub fn spawn(interval: Duration, tx: mpsc::Sender<()>) {
    tokio::spawn(async move {
        info!(interval_secs = interval.as_secs(), "poller started");
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // skip immediate first tick
        loop {
            ticker.tick().await;
            match tx.try_send(()) {
                Ok(_) => debug!("poller: rescan queued"),
                Err(mpsc::error::TrySendError::Full(_)) => {
                    debug!("poller: rescan already pending");
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    debug!("poller: scan channel closed, exiting");
                    break;
                }
            }
        }
    });
}
