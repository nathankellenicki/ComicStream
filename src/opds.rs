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
    let _ = write!(
        s,
        "<feed xmlns=\"{}\" xmlns:opds=\"{}\" xmlns:pse=\"{}\">\n",
        ATOM_NS, OPDS_NS, PSE_NS
    );

    let self_type = if ctx.kind_acquisition { ACQ_TYPE } else { NAV_TYPE };
    let _ = write!(s, "  <id>{}</id>\n", esc(&ctx.feed_id));
    let _ = write!(s, "  <title>{}</title>\n", esc(&ctx.title));
    let _ = write!(s, "  <updated>{}</updated>\n", ctx.updated.to_rfc3339());
    s.push_str("  <author><name>ComicStream</name></author>\n");
    let _ = write!(
        s,
        "  <link rel=\"self\" href=\"{}\" type=\"{}\"/>\n",
        esc(ctx.self_href),
        self_type
    );
    let _ = write!(
        s,
        "  <link rel=\"start\" href=\"/opds\" type=\"{}\"/>\n",
        NAV_TYPE
    );
    if let Some(up) = ctx.up_href {
        let _ = write!(
            s,
            "  <link rel=\"up\" href=\"{}\" type=\"{}\"/>\n",
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
    let _ = write!(s, "  <entry>\n");
    let _ = write!(s, "    <id>urn:comicstream:folder:{}</id>\n", f.id);
    let _ = write!(s, "    <title>{}</title>\n", esc(&f.name));
    let _ = write!(s, "    <updated>{}</updated>\n", updated);
    let _ = write!(
        s,
        "    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/folders/{}/cover\" type=\"image/jpeg\"/>\n",
        f.id
    );
    let _ = write!(
        s,
        "    <link rel=\"subsection\" href=\"/opds/folders/{}\" type=\"{}\"/>\n",
        f.id, NAV_TYPE
    );
    let _ = write!(s, "  </entry>\n");
}

fn write_book_entry(s: &mut String, b: &Book) {
    let updated = ts_to_rfc3339(b.added_at.max(b.mtime));
    let _ = write!(s, "  <entry>\n");
    let _ = write!(s, "    <id>urn:comicstream:book:{}</id>\n", esc(&b.hash));
    let _ = write!(s, "    <title>{}</title>\n", esc(&b.name));
    let _ = write!(s, "    <updated>{}</updated>\n", updated);
    let _ = write!(
        s,
        "    <link rel=\"http://opds-spec.org/image\" href=\"/books/{}/cover\" type=\"image/jpeg\"/>\n",
        esc(&b.hash)
    );
    let _ = write!(
        s,
        "    <link rel=\"http://opds-spec.org/image/thumbnail\" href=\"/books/{}/thumbnail\" type=\"image/jpeg\"/>\n",
        esc(&b.hash)
    );
    let _ = write!(
        s,
        "    <link rel=\"http://vaemendis.net/opds-pse/stream\" href=\"/books/{}/pages/{{pageNumber}}\" type=\"image/jpeg\" pse:count=\"{}\"/>\n",
        esc(&b.hash),
        b.page_count
    );
    let mime = book_mime(&b.format);
    let _ = write!(
        s,
        "    <link rel=\"http://opds-spec.org/acquisition\" href=\"/books/{}/file\" type=\"{}\" length=\"{}\"/>\n",
        esc(&b.hash),
        mime,
        b.file_size
    );
    let _ = write!(s, "  </entry>\n");
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
