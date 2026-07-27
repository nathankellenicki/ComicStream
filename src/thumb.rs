// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::ffi::OsString;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Result};
use image::imageops::FilterType;
use image::ImageFormat;

use crate::archive::MAX_PAGE_BYTES;

const COVER_WIDTH: u32 = 400;
pub const MAX_PAGE_THUMB_WIDTH: u32 = 1600;

/// The only page-thumbnail widths that are ever generated.
///
/// Requested widths are snapped onto this ladder instead of being honoured
/// exactly. Every distinct width is a permanent file in the cache plus a full
/// decode-and-resize, so honouring arbitrary values lets a single client fill
/// the disk and burn CPU just by iterating `width=`. Six rungs cover phone,
/// tablet and desktop-retina sizes while bounding the cache at six variants per
/// page instead of MAX_PAGE_THUMB_WIDTH of them.
pub const PAGE_THUMB_WIDTHS: &[u32] = &[150, 300, 600, 900, 1200, MAX_PAGE_THUMB_WIDTH];

/// Snap a requested width onto [`PAGE_THUMB_WIDTHS`].
///
/// Rounds *up* to the next rung so a client never gets something blurrier than
/// it asked for, and caps at [`MAX_PAGE_THUMB_WIDTH`]. A request for 0 lands on
/// the smallest rung, which also keeps degenerate zero-width resizes out of the
/// encoder.
pub fn snap_width(requested: u32) -> u32 {
    PAGE_THUMB_WIDTHS
        .iter()
        .copied()
        .find(|w| *w >= requested)
        .unwrap_or(MAX_PAGE_THUMB_WIDTH)
}
/// Cap on decoded pixel area. This is a *secondary* guard: `image` 0.25 already
/// applies `Limits::default()` (512 MiB `max_alloc`) inside `load_from_memory`
/// and reserves against it before allocating, so a decompression bomb is capped
/// there first. This bound is tighter and expressed in pixels rather than bytes;
/// the *input* is separately gated by MAX_PAGE_BYTES.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

pub fn cache_path(data_dir: &Path, hash: &str) -> PathBuf {
    data_dir.join("thumbs").join(format!("{}.jpg", hash))
}

pub fn page_thumb_path(data_dir: &Path, hash: &str, page_index: usize, width: u32) -> PathBuf {
    data_dir
        .join("page_thumbs")
        .join(hash)
        .join(format!("{}_{}.jpg", page_index, width))
}

pub fn ensure_thumbnail(data_dir: &Path, hash: &str, source_bytes: &[u8]) -> Result<PathBuf> {
    let out = cache_path(data_dir, hash);
    if out.exists() {
        return Ok(out);
    }
    write_resized(&out, source_bytes, COVER_WIDTH)?;
    Ok(out)
}

pub fn ensure_page_thumbnail(
    data_dir: &Path,
    hash: &str,
    page_index: usize,
    width: u32,
    source_bytes: &[u8],
) -> Result<PathBuf> {
    let out = page_thumb_path(data_dir, hash, page_index, width);
    if out.exists() {
        return Ok(out);
    }
    write_resized(&out, source_bytes, width)?;
    Ok(out)
}

fn write_resized(out: &Path, source_bytes: &[u8], width: u32) -> Result<()> {
    if source_bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(anyhow!(
            "thumbnail source {} bytes exceeds MAX_PAGE_BYTES",
            source_bytes.len()
        ));
    }

    let parent = out
        .parent()
        .ok_or_else(|| anyhow!("thumbnail output path {} has no parent", out.display()))?;
    std::fs::create_dir_all(parent)?;

    let img = image::load_from_memory(source_bytes)?;
    if u64::from(img.width()) * u64::from(img.height()) > MAX_PIXELS {
        return Err(anyhow!(
            "decoded image {}x{} exceeds MAX_PIXELS",
            img.width(),
            img.height()
        ));
    }
    let resized = if img.width() > width {
        let h = img.height() as f32 * (width as f32 / img.width() as f32);
        img.resize(width, h as u32, FilterType::Triangle)
    } else {
        img
    };

    let mut buf = Cursor::new(Vec::new());
    resized.write_to(&mut buf, ImageFormat::Jpeg)?;

    // Write to a unique sibling temp file then atomically rename. Without this,
    // concurrent generators for the same destination race: writer B's
    // truncate-on-create can land between writer A's write completion and A's
    // response read, exposing a 0-byte file to the client.
    let tmp = tmp_path_for(out);
    let cleanup = TmpGuard(&tmp);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&buf.into_inner())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, out)?;
    std::mem::forget(cleanup);
    Ok(())
}

fn tmp_path_for(out: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name: OsString = out.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}.{}", std::process::id(), n));
    out.with_file_name(name)
}

/// Removes a temp file if `write_resized` fails partway through. Forgotten on
/// the success path so the rename target isn't deleted.
struct TmpGuard<'a>(&'a Path);

impl Drop for TmpGuard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}
