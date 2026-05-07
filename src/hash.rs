// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use xxhash_rust::xxh3::Xxh3;

const SAMPLE: usize = 64 * 1024;

pub fn file_hash(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();

    let mut h = Xxh3::new();
    h.update(&len.to_le_bytes());

    let mut buf = vec![0u8; SAMPLE.min(len as usize)];
    if !buf.is_empty() {
        f.read_exact(&mut buf)?;
        h.update(&buf);
    }

    if len > SAMPLE as u64 * 2 {
        let mut tail = vec![0u8; SAMPLE];
        f.seek(SeekFrom::End(-(SAMPLE as i64)))?;
        f.read_exact(&mut tail)?;
        h.update(&tail);
    } else if len > SAMPLE as u64 {
        let remaining = (len as usize) - SAMPLE;
        let mut tail = vec![0u8; remaining];
        f.read_exact(&mut tail)?;
        h.update(&tail);
    }

    Ok(format!("{:016x}", h.digest()))
}
