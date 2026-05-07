PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE folder (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id     INTEGER REFERENCES folder(id) ON DELETE CASCADE,
    path          TEXT    NOT NULL UNIQUE,
    name          TEXT    NOT NULL,
    sort_key      TEXT    NOT NULL,
    cover_path    TEXT,
    cover_version TEXT,
    slug          TEXT    NOT NULL UNIQUE,
    mtime         INTEGER NOT NULL,
    seen_at       INTEGER NOT NULL
);

CREATE INDEX idx_folder_parent ON folder(parent_id, sort_key);

CREATE TABLE book (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id    INTEGER NOT NULL REFERENCES folder(id) ON DELETE CASCADE,
    hash         TEXT    NOT NULL UNIQUE,
    path         TEXT    NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    sort_key     TEXT    NOT NULL,
    format       TEXT    NOT NULL,
    file_size    INTEGER NOT NULL,
    mtime        INTEGER NOT NULL,
    page_count   INTEGER NOT NULL,
    added_at     INTEGER NOT NULL,
    seen_at      INTEGER NOT NULL
);

CREATE INDEX idx_book_folder ON book(folder_id, sort_key);
CREATE INDEX idx_book_hash ON book(hash);
