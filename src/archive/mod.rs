// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Result};

mod cbr;
mod cbz;

pub use cbr::Cbr;
pub use cbz::Cbz;

#[derive(Debug, Clone)]
pub struct PageEntry {
    pub name: String,
    pub mime: &'static str,
}

pub trait Book: Send + Sync {
    fn page_count(&self) -> usize;
    fn read_page(&self, index: usize) -> Result<(Vec<u8>, &'static str)>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Rar,
}

pub fn detect(path: &Path) -> Result<Kind> {
    let mut f = File::open(path)?;
    let mut buf = [0u8; 8];
    let n = f.read(&mut buf)?;
    let head = &buf[..n];
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        Ok(Kind::Zip)
    } else if head.starts_with(b"Rar!\x1a\x07\x00") || head.starts_with(b"Rar!\x1a\x07\x01") {
        Ok(Kind::Rar)
    } else {
        Err(anyhow!("unknown archive magic in {}", path.display()))
    }
}

pub fn open(path: &Path) -> Result<Box<dyn Book>> {
    match detect(path)? {
        Kind::Zip => Ok(Box::new(Cbz::open(path)?)),
        Kind::Rar => Ok(Box::new(Cbr::open(path)?)),
    }
}

pub fn is_supported(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("cbz") | Some("zip") | Some("cbr") | Some("rar")
    )
}

pub(crate) fn image_mime(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    let ext = std::path::Path::new(&lower).extension()?.to_str()?;
    Some(match ext {
        "jpg" | "jpeg" | "jpe" | "jfif" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}
