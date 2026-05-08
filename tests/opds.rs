// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Nathan Kellenicki

use chrono::{TimeZone, Utc};

use comicstream::models::{Book, Folder};
use comicstream::opds::{book_mime, build_feed, FeedCtx};

#[test]
fn book_mime_maps_known_formats() {
    assert_eq!(book_mime("cbz"), "application/vnd.comicbook+zip");
    assert_eq!(book_mime("zip"), "application/vnd.comicbook+zip");
    assert_eq!(book_mime("cbr"), "application/vnd.comicbook-rar");
    assert_eq!(book_mime("rar"), "application/vnd.comicbook-rar");
    assert_eq!(book_mime("anything-else"), "application/octet-stream");
}

fn folder(id: i64, parent_id: Option<i64>, name: &str) -> Folder {
    Folder {
        id,
        parent_id,
        path: format!("/library/{}", name),
        name: name.into(),
        sort_key: name.to_lowercase(),
        cover_path: None,
        mtime: 1_700_000_000,
        seen_at: 1_700_000_000,
        cover_version: Some(format!("v-{}", id)),
        description: None,
        slug: format!("slug{}", id),
    }
}

fn folder_with_description(id: i64, parent_id: Option<i64>, name: &str, desc: &str) -> Folder {
    let mut f = folder(id, parent_id, name);
    f.description = Some(desc.into());
    f
}

fn book(id: i64, hash: &str, name: &str, page_count: i64, format: &str) -> Book {
    Book {
        id,
        folder_id: 1,
        hash: hash.into(),
        path: format!("/library/{}", name),
        name: name.into(),
        sort_key: name.to_lowercase(),
        format: format.into(),
        file_size: 12345,
        mtime: 1_700_000_000,
        page_count,
        added_at: 1_700_000_000,
        seen_at: 1_700_000_000,
    }
}

fn ctx<'a>(self_href: &'a str, up: Option<&'a str>, kind_acquisition: bool) -> FeedCtx<'a> {
    FeedCtx {
        self_href,
        up_href: up,
        feed_id: "urn:comicstream:folder:1".into(),
        title: "Library".into(),
        updated: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        kind_acquisition,
    }
}

#[test]
fn navigation_feed_lists_subfolders_with_subsection_links() {
    let subs = vec![folder(2, Some(1), "DC"), folder(3, Some(1), "Marvel")];
    let xml = build_feed(&ctx("/opds", None, false), &subs, &[]);

    assert!(xml.starts_with("<?xml version=\"1.0\""));
    assert!(xml.contains("<title>Library</title>"));
    assert!(xml.contains("rel=\"subsection\" href=\"/opds/folders/slug2\""));
    assert!(xml.contains("rel=\"subsection\" href=\"/opds/folders/slug3\""));
    assert!(xml.contains("href=\"/folders/slug2/cover?v=v-2\""));
    assert!(xml.contains("href=\"/folders/slug3/cover?v=v-3\""));
    assert!(xml.contains("urn:comicstream:folder:slug2"));
    assert!(xml.contains("urn:comicstream:folder:slug3"));
    assert!(xml.contains("kind=navigation"));
    assert!(!xml.contains("opds-pse/stream"));
}

#[test]
fn acquisition_feed_includes_pse_link_with_literal_pagenumber_template() {
    let books = vec![book(1, "abc123", "Issue 1", 42, "cbz")];
    let xml = build_feed(&ctx("/opds/folders/5", Some("/opds"), true), &[], &books);

    assert!(xml.contains("kind=acquisition"));
    assert!(xml.contains("urn:comicstream:book:abc123"));
    assert!(xml.contains(
        "rel=\"http://vaemendis.net/opds-pse/stream\" \
         href=\"/books/abc123/pages/{pageNumber}\" \
         type=\"image/jpeg\" \
         pse:count=\"42\""
    ));
    assert!(xml.contains("rel=\"http://opds-spec.org/acquisition\""));
    assert!(xml.contains("application/vnd.comicbook+zip"));
}

#[test]
fn cbr_book_uses_cbr_mime_type() {
    let books = vec![book(1, "deadbeef", "RAR Issue", 5, "cbr")];
    let xml = build_feed(&ctx("/opds/folders/5", None, true), &[], &books);
    assert!(xml.contains("application/vnd.comicbook-rar"));
}

#[test]
fn special_characters_in_titles_are_xml_escaped() {
    let books = vec![book(
        1,
        "f00d",
        "Pirates & \"Privateers\" <Issue 1>",
        10,
        "cbz",
    )];
    let xml = build_feed(&ctx("/opds", None, true), &[], &books);

    assert!(xml.contains("Pirates &amp; &quot;Privateers&quot; &lt;Issue 1&gt;"));
    // and the raw form must NOT appear
    assert!(!xml.contains("Pirates & \"Privateers\" <Issue 1>"));
}

#[test]
fn root_feed_omits_up_link() {
    let xml = build_feed(&ctx("/opds", None, false), &[], &[]);
    assert!(!xml.contains("rel=\"up\""));
}

#[test]
fn every_feed_advertises_opensearch_descriptor() {
    let xml = build_feed(&ctx("/opds", None, false), &[], &[]);
    assert!(xml.contains(
        "rel=\"search\" type=\"application/opensearchdescription+xml\" \
         href=\"/opds/opensearch.xml\""
    ));
}

#[test]
fn folder_without_description_omits_summary_element() {
    let subs = vec![folder(2, Some(1), "Plain")];
    let xml = build_feed(&ctx("/opds", None, false), &subs, &[]);
    assert!(!xml.contains("<summary"));
}

#[test]
fn folder_with_description_emits_escaped_summary() {
    let subs = vec![folder_with_description(
        2,
        Some(1),
        "Has Notes",
        "Vol. 1 & 2 — \"complete\" run",
    )];
    let xml = build_feed(&ctx("/opds", None, false), &subs, &[]);
    assert!(xml.contains("<summary type=\"text\">"));
    assert!(xml.contains("Vol. 1 &amp; 2 — &quot;complete&quot; run"));
    // raw form must not appear
    assert!(!xml.contains("Vol. 1 & 2 — \"complete\" run"));
}

#[test]
fn nested_feed_includes_up_link() {
    let xml = build_feed(
        &ctx("/opds/folders/3", Some("/opds/folders/2"), false),
        &[],
        &[],
    );
    assert!(xml.contains("rel=\"up\" href=\"/opds/folders/2\""));
}
