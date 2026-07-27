-- Drop the UNIQUE constraint on book.hash.
--
-- Identical content stored at two paths is a legitimate library layout, but
-- UNIQUE(hash) made the second copy unrepresentable: the scanner refused it and
-- the book silently never appeared in the catalog. Rows are identified by
-- `path` (still UNIQUE), so duplicate hashes are safe — the scanner never
-- selects a row by hash alone.
--
-- SQLite cannot drop a constraint in place, so the table is rebuilt.

CREATE TABLE book_new (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id    INTEGER NOT NULL REFERENCES folder(id) ON DELETE CASCADE,
    hash         TEXT    NOT NULL,
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

INSERT INTO book_new (
    id, folder_id, hash, path, name, sort_key,
    format, file_size, mtime, page_count, added_at, seen_at
)
SELECT
    id, folder_id, hash, path, name, sort_key,
    format, file_size, mtime, page_count, added_at, seen_at
FROM book;

DROP TABLE book;

ALTER TABLE book_new RENAME TO book;

CREATE INDEX idx_book_folder ON book(folder_id, sort_key);
CREATE INDEX idx_book_hash ON book(hash);
