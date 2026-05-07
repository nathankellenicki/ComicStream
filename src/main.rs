// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use comicstream::{db, poller, routes, scan, state, watcher};

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "comicstream", about = "ComicStream - A lightweight, zero-dependency, OPDS+PSE comic server that preserves your folder hierarchy")]
struct Cli {
    /// Library root directory containing your comics
    #[arg(long, env = "COMICSTREAM_LIBRARY")]
    library: PathBuf,

    /// Address to bind
    #[arg(long, env = "COMICSTREAM_BIND", default_value = "0.0.0.0:8080")]
    bind: SocketAddr,

    /// Data directory for the database and thumbnail cache
    #[arg(long, env = "COMICSTREAM_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,

    /// Display name for the library root in OPDS feeds. Defaults to the library directory's basename.
    #[arg(long, env = "COMICSTREAM_LIBRARY_NAME")]
    library_name: Option<String>,

    /// Disable the filesystem watcher (use this on SMB/NFS, where notify events don't fire reliably)
    #[arg(
        long,
        env = "COMICSTREAM_NO_WATCH",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    no_watch: bool,

    /// Watcher debounce: events coalesced within this window trigger one rescan
    #[arg(long, env = "COMICSTREAM_WATCH_DEBOUNCE", default_value = "5s", value_parser = parse_duration)]
    watch_debounce: Duration,

    /// Periodic rescan interval (e.g. "5m", "30s", "1h"). Disabled if not set.
    #[arg(long, env = "COMICSTREAM_SCAN_INTERVAL", value_parser = parse_duration)]
    scan_interval: Option<Duration>,

    /// Generate any missing thumbnails at the end of each scan instead of waiting for them to be requested
    #[arg(
        long,
        env = "COMICSTREAM_PREWARM_THUMBNAILS",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    prewarm_thumbnails: bool,

    /// Default width (px) for `Prefer: variant=thumbnail` requests that don't specify a width. Also the width used during prewarm.
    #[arg(long, env = "COMICSTREAM_PAGE_THUMB_WIDTH", default_value_t = 300)]
    page_thumb_width: u32,

    /// Log full request details (method, URI, all headers) for every request. Useful for diagnosing client behavior.
    #[arg(
        long,
        env = "COMICSTREAM_LOG_REQUESTS",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    log_requests: bool,

    /// Skip the initial scan and disable all rescan triggers (serve whatever is already in the DB)
    #[arg(
        long,
        env = "COMICSTREAM_NO_SCAN",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    no_scan: bool,
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=info")),
        )
        .init();

    let cli = Cli::parse();

    if !cli.library.is_dir() {
        anyhow::bail!("library {} is not a directory", cli.library.display());
    }

    std::fs::create_dir_all(&cli.data_dir)
        .with_context(|| format!("creating data dir {}", cli.data_dir.display()))?;

    let db_path = cli.data_dir.join("comicstream.db");
    let pool = db::open(&db_path).await?;

    let scan_tx = if cli.no_scan {
        let (tx, _rx) = mpsc::channel::<()>(1);
        info!("scanning disabled");
        tx
    } else {
        let tx = scan::spawn_loop(
            pool.clone(),
            scan::ScanOptions {
                library: cli.library.clone(),
                library_name: cli.library_name.clone(),
                data_dir: cli.data_dir.clone(),
                prewarm_thumbnails: cli.prewarm_thumbnails,
                page_thumb_width: cli.page_thumb_width,
            },
        );

        if cli.no_watch {
            info!("watcher disabled");
        } else if let Err(e) = watcher::spawn(cli.library.clone(), cli.watch_debounce, tx.clone()) {
            warn!("watcher could not start: {:#}", e);
        }

        if let Some(interval) = cli.scan_interval {
            poller::spawn(interval, tx.clone());
        }

        tx
    };

    let state = state::AppState {
        pool,
        data_dir: Arc::new(cli.data_dir.clone()),
        scan_tx,
        page_thumb_default_width: cli.page_thumb_width,
    };

    let mut app = routes::router(state);
    if cli.log_requests {
        app = app.layer(axum::middleware::from_fn(routes::log_request));
        info!("request logging enabled");
    }
    let app = app.layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    info!(bind = %cli.bind, library = %cli.library.display(), "ComicStream listening");
    axum::serve(listener, app).await?;
    Ok(())
}
