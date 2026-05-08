// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use image::imageops::FilterType;
use image::ImageFormat;

use crate::archive::MAX_PAGE_BYTES;

const COVER_WIDTH: u32 = 400;
pub const MAX_PAGE_THUMB_WIDTH: u32 = 1600;
/// Cap on decoded pixel area. `image` 0.25 has no decoder-side `Limits`, so
/// this is a post-decode guard — we still gate the *input* via MAX_PAGE_BYTES.
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

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }

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
    std::fs::write(out, buf.into_inner())?;
    Ok(())
}
