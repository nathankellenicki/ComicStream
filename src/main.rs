// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use comicstream::{archive, auth, db, poller, rate_limit, routes, scan, state, watcher};

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
#[command(
    name = "comicstream",
    about = "ComicStream - A lightweight, zero-dependency, OPDS+PSE comic server that preserves your folder hierarchy"
)]
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

    /// Log request details (method, URI, headers with secrets redacted) for every request. Useful for diagnosing client behavior.
    #[arg(
        long,
        env = "COMICSTREAM_LOG_REQUESTS",
        num_args = 0..=1,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    log_requests: bool,

    /// Maximum concurrent in-flight HTTP requests. Excess requests queue.
    #[arg(
        long,
        env = "COMICSTREAM_MAX_CONCURRENT_REQUESTS",
        default_value_t = 32
    )]
    max_concurrent_requests: usize,

    /// Number of opened archives to keep cached in memory. Each entry is small
    /// (a path + sorted page list); raising this avoids re-parsing archives on
    /// successive page requests, which especially helps CBR.
    #[arg(long, env = "COMICSTREAM_ARCHIVE_CACHE_SIZE", default_value_t = 256)]
    archive_cache_size: usize,

    /// Username for HTTP Basic auth. If set, --auth-password must also be set;
    /// all routes except /health then require authentication. If unset, no auth.
    #[arg(long, env = "COMICSTREAM_AUTH_USERNAME")]
    auth_username: Option<String>,

    /// Password for HTTP Basic auth. Pair with --auth-username. Prefer setting
    /// via env (COMICSTREAM_AUTH_PASSWORD) so it doesn't show up in `ps` output.
    #[arg(long, env = "COMICSTREAM_AUTH_PASSWORD", hide_env_values = true)]
    auth_password: Option<String>,

    /// Failed-auth attempts from a single IP within `--rate-limit-window` that trigger a block.
    #[arg(long, env = "COMICSTREAM_RATE_LIMIT_ATTEMPTS", default_value_t = 10)]
    rate_limit_attempts: u32,

    /// Window in which failed attempts accumulate (e.g. "60s", "5m").
    #[arg(long, env = "COMICSTREAM_RATE_LIMIT_WINDOW", default_value = "60s", value_parser = parse_duration)]
    rate_limit_window: Duration,

    /// How long an IP stays blocked after exceeding the threshold (e.g. "5m", "1h").
    #[arg(long, env = "COMICSTREAM_RATE_LIMIT_BLOCK", default_value = "5m", value_parser = parse_duration)]
    rate_limit_block: Duration,

    /// CIDR ranges (repeatable) of trusted reverse proxies. When the TCP peer is
    /// in one of these ranges, X-Forwarded-For is consulted to determine the
    /// real client IP for rate limiting. Without this set, the rate limiter
    /// keys on the TCP peer IP only — which collapses to a single bucket
    /// behind a reverse proxy.
    #[arg(
        long,
        env = "COMICSTREAM_TRUSTED_PROXY",
        value_delimiter = ',',
        num_args = 0..,
    )]
    trusted_proxy: Vec<ipnet::IpNet>,

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

    // Canonicalize once at startup so AppState and the scanner share the same
    // prefix used for path-containment checks at request time.
    let library_root = cli
        .library
        .canonicalize()
        .with_context(|| format!("canonicalizing library {}", cli.library.display()))?;

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
                library: library_root.clone(),
                library_name: cli.library_name.clone(),
                data_dir: cli.data_dir.clone(),
                prewarm_thumbnails: cli.prewarm_thumbnails,
                page_thumb_width: cli.page_thumb_width,
            },
        );

        if cli.no_watch {
            info!("watcher disabled");
        } else if let Err(e) = watcher::spawn(library_root.clone(), cli.watch_debounce, tx.clone())
        {
            warn!("watcher could not start: {:#}", e);
        }

        if let Some(interval) = cli.scan_interval {
            poller::spawn(interval, tx.clone());
        }

        tx
    };

    let archive_cache = Arc::new(archive::Cache::new(cli.archive_cache_size));

    let auth_setup = match (cli.auth_username.as_deref(), cli.auth_password.as_deref()) {
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => {
            info!(username = u, "HTTP Basic auth enabled");
            warn!(
                "HTTP Basic auth sends credentials on every request; use HTTPS directly or put ComicStream behind a TLS-terminating reverse proxy"
            );
            info!(
                attempts = cli.rate_limit_attempts,
                window_secs = cli.rate_limit_window.as_secs(),
                block_secs = cli.rate_limit_block.as_secs(),
                trusted_proxies = cli.trusted_proxy.len(),
                "auth rate limit configured"
            );
            Some(routes::AuthSetup {
                creds: Arc::new(auth::Credentials::new(u, p)),
                limiter: Arc::new(rate_limit::Limiter::new(
                    cli.rate_limit_attempts,
                    cli.rate_limit_window,
                    cli.rate_limit_block,
                )),
                proxy: Arc::new(comicstream::peer_ip::ProxyConfig::new(
                    cli.trusted_proxy.clone(),
                )),
            })
        }
        (None, None) | (Some(""), None) | (None, Some("")) | (Some(""), Some("")) => {
            info!("HTTP Basic auth disabled (no credentials configured)");
            None
        }
        _ => anyhow::bail!("--auth-username and --auth-password must both be set, or both omitted"),
    };

    let state = state::AppState {
        pool,
        data_dir: Arc::new(cli.data_dir.clone()),
        library_root: Arc::new(library_root.clone()),
        scan_tx,
        page_thumb_default_width: cli.page_thumb_width,
        archive_cache,
    };

    let mut app = routes::router(state, auth_setup);
    if cli.log_requests {
        app = app.layer(axum::middleware::from_fn(routes::log_request));
        info!("request logging enabled");
    }
    let app = app
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower::limit::ConcurrencyLimitLayer::new(
            cli.max_concurrent_requests,
        ));
    info!(
        max_concurrent_requests = cli.max_concurrent_requests,
        archive_cache_size = cli.archive_cache_size,
        "performance limits"
    );

    let listener = tokio::net::TcpListener::bind(cli.bind).await?;
    info!(bind = %cli.bind, library = %cli.library.display(), "ComicStream listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
