// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use zip::ZipArchive;

use crate::natsort;

use super::{image_mime, Book, PageEntry};

pub struct Cbz {
    path: PathBuf,
    pages: Vec<PageEntry>,
}

impl Cbz {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut zip = ZipArchive::new(BufReader::new(f))?;

        let mut pages: Vec<PageEntry> = Vec::new();
        for i in 0..zip.len() {
            let entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
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

impl Book for Cbz {
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    fn read_page(&self, index: usize) -> Result<(Vec<u8>, &'static str)> {
        let entry = self
            .pages
            .get(index)
            .ok_or_else(|| anyhow!("page index {} out of range", index))?;
        let f = File::open(&self.path)?;
        let mut zip = ZipArchive::new(BufReader::new(f))?;
        let mut zf = zip.by_name(&entry.name)?;
        let mut buf = Vec::with_capacity(zf.size() as usize);
        zf.read_to_end(&mut buf)?;
        Ok((buf, entry.mime))
    }
}
