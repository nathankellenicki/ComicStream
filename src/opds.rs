// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use std::fmt::Write;

use chrono::{DateTime, Utc};

use crate::models::{Book, Folder};

const ATOM_NS: &str = "http://www.w3.org/2005/Atom";
const OPDS_NS: &str = "http://opds-spec.org/2010/catalog";
const PSE_NS: &str = "http://vaemendis.net/opds-pse/ns";

const NAV_TYPE: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const ACQ_TYPE: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";

pub struct FeedCtx<'a> {
    pub self_href: &'a str,
    pub up_href: Option<&'a str>,
    pub feed_id: String,
    pub title: String,
    pub updated: DateTime<Utc>,
    pub kind_acquisition: bool,
}

pub fn build_feed(ctx: &FeedCtx, subfolders: &[Folder], books: &[Book]) -> String {
    let mut s = String::with_capacity(2048);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        s,
        "<feed xmlns=\"{}\" xmlns:opds=\"{}\" xmlns:pse=\"{}\">",
        ATOM_NS, OPDS_NS, PSE_NS
    );

    let self_type = if ctx.kind_acquisition {
        ACQ_TYPE
    } else {
        NAV_TYPE
    };
    let _ = writeln!(s, "  <id>{}</id>", esc(&ctx.feed_id));
    let _ = writeln!(s, "  <title>{}</title>", esc(&ctx.title));
    let _ = writeln!(s, "  <updated>{}</updated>", ctx.updated.to_rfc3339());
    s.push_str("  <author><name>ComicStream</name></author>\n");
    let _ = writeln!(
        s,
        "  <link rel=\"self\" href=\"{}\" type=\"{}\"/>",
        esc(ctx.self_href),
        self_type
    );
    let _ = writeln!(
        s,
        "  <link rel=\"start\" href=\"/opds\" type=\"{}\"/>",
        NAV_TYPE
    );
    if let Some(up) = ctx.up_href {
        let _ = writeln!(
            s,
            "  <link rel=\"up\" href=\"{}\" type=\"{}\"/>",
            esc(up),
            NAV_TYPE
        );
    }

    for f in subfolders {
        write_folder_entry(&mut s, f);
    }
    for b in books {
        write_book_entry(&mut s, b);
    }

    s.push_str("</feed>\n");
    s
}

fn write_folder_entry(s: &mut String, f: &Folder) {
    let updated = ts_to_rfc3339(f.mtime);
    let _ = writeln!(s, "  <entry>");
    let _ = writeln!(s, "    <id>urn:comicstream:folder:{}</id>", f.id);
    let _ = writeln!(s, "    <title>{}</title>", esc(&f.name));
    let _ = writeln!(s, "    <updated>{}</updated>", updated);
    let _ = writeln!(
        s,
        "    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/folders/{}/cover\" type=\"image/jpeg\"/>",
        f.id
    );
    let _ = writeln!(
        s,
        "    <link rel=\"subsection\" href=\"/opds/folders/{}\" type=\"{}\"/>",
        f.id, NAV_TYPE
    );
    let _ = writeln!(s, "  </entry>");
}

fn write_book_entry(s: &mut String, b: &Book) {
    let updated = ts_to_rfc3339(b.added_at.max(b.mtime));
    let _ = writeln!(s, "  <entry>");
    let _ = writeln!(s, "    <id>urn:comicstream:book:{}</id>", esc(&b.hash));
    let _ = writeln!(s, "    <title>{}</title>", esc(&b.name));
    let _ = writeln!(s, "    <updated>{}</updated>", updated);
    let _ = writeln!(
        s,
        "    <link rel=\"http://opds-spec.org/image\" href=\"/books/{}/cover\" type=\"image/jpeg\"/>",
        esc(&b.hash)
    );
    let _ = writeln!(
        s,
        "    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/books/{}/thumbnail\" type=\"image/jpeg\"/>",
        esc(&b.hash)
    );
    let _ = writeln!(
        s,
        "    <link rel=\"http://vaemendis.net/opds-pse/stream\" href=\"/books/{}/pages/{{pageNumber}}\" type=\"image/jpeg\" pse:count=\"{}\"/>",
        esc(&b.hash),
        b.page_count
    );
    let mime = book_mime(&b.format);
    let _ = writeln!(
        s,
        "    <link rel=\"http://opds-spec.org/acquisition\" href=\"/books/{}/file\" type=\"{}\" length=\"{}\"/>",
        esc(&b.hash),
        mime,
        b.file_size
    );
    let _ = writeln!(s, "  </entry>");
}

pub fn book_mime(format: &str) -> &'static str {
    match format {
        "cbz" | "zip" => "application/vnd.comicbook+zip",
        "cbr" | "rar" => "application/vnd.comicbook-rar",
        _ => "application/octet-stream",
    }
}

fn ts_to_rfc3339(secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(secs, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}
