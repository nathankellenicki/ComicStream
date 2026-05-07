// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use unrar::Archive;

use crate::natsort;

use super::{image_mime, Book, PageEntry};

pub struct Cbr {
    path: PathBuf,
    pages: Vec<PageEntry>,
}

impl Cbr {
    pub fn open(path: &Path) -> Result<Self> {
        let mut pages: Vec<PageEntry> = Vec::new();

        let listing = Archive::new(path)
            .open_for_listing()
            .with_context(|| format!("opening RAR {}", path.display()))?;

        for entry in listing {
            let entry =
                entry.with_context(|| format!("reading RAR header in {}", path.display()))?;
            if entry.is_directory() {
                continue;
            }
            let name = entry.filename.to_string_lossy().into_owned();
            if let Some(mime) = image_mime(&name) {
                pages.push(PageEntry { name, mime });
            }
        }

        pages.sort_by(|a, b| natsort::cmp(&a.name, &b.name));

        Ok(Self {
            path: path.to_path_buf(),
            pages,
        })
    }
}

impl Book for Cbr {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn read_page(&self, index: usize) -> Result<(Vec<u8>, &'static str)> {
        let entry = self
            .pages
            .get(index)
            .ok_or_else(|| anyhow!("page index {} out of range", index))?;
        let target = PathBuf::from(&entry.name);

        let mut archive = Archive::new(&self.path)
            .open_for_processing()
            .with_context(|| format!("opening RAR {}", self.path.display()))?;

        while let Some(header) = archive.read_header()? {
            let matches = header.entry().filename == target;
            archive = if matches {
                let (data, _rest) = header.read()?;
                return Ok((data, entry.mime));
            } else {
                header.skip()?
            };
        }

        Err(anyhow!(
            "page {} (entry {}) not found in {}",
            index,
            entry.name,
            self.path.display()
        ))
    }
}
