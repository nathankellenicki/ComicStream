# ComicStream

A lightweight, zero-dependency OPDS+PSE comic server that preserves your folder hierarchy.

Komga, Kavita, and friends collapse a library into a flat series/issues model. If you keep your comics in nested folders like `Comics/Marvel/X-Men/Uncanny X-Men/`, they don't show up that way in your reader. ComicStream serves the tree as it sits on disk, however deep.

Reads CBZ and CBR. Single binary on macOS and Linux, or a ~19 MB Docker image.

## Running

Docker:

```sh
docker compose up -d
```

Edit the `volumes:` line in `docker-compose.yml` to point at your library first.

Native:

```sh
comicstream --library /path/to/comics
```

Then point any OPDS reader at `http://your-host:8080/`. Tested with [Panels](https://panels.app/) on iOS and iPadOS.

## Configuration

Every flag has an equivalent env var.

| Flag | Env | Default |
| --- | --- | --- |
| `--library` | `COMICSTREAM_LIBRARY` | required |
| `--data-dir` | `COMICSTREAM_DATA_DIR` | `./data` |
| `--bind` | `COMICSTREAM_BIND` | `0.0.0.0:8080` |
| `--library-name` | `COMICSTREAM_LIBRARY_NAME` | basename of `--library` |
| `--no-watch` | `COMICSTREAM_NO_WATCH` | false |
| `--scan-interval` | `COMICSTREAM_SCAN_INTERVAL` | unset |
| `--auth-username` | `COMICSTREAM_AUTH_USERNAME` | unset (no auth) |
| `--auth-password` | `COMICSTREAM_AUTH_PASSWORD` | unset (no auth) |

`comicstream --help` lists the rest, including thumbnail and tuning options.

## Detecting new comics

ComicStream watches your library folder and refreshes the catalog as you add or remove files.

Filesystem watching doesn't work over SMB or NFS. If your library lives on a network share, disable the watcher and turn on periodic scans:

```yaml
COMICSTREAM_NO_WATCH: "1"
COMICSTREAM_SCAN_INTERVAL: "5m"
```

To force a refresh at any time:

```sh
curl -X POST http://localhost:8080/admin/rescan
```

## Folder descriptions

Drop a `description.txt` into any folder and ComicStream will surface its contents as the folder's description in OPDS feeds. Plain text only; the file is read at scan time, capped at 16 KB.

## Search

The search field in your OPDS reader matches against folder names and comic filenames anywhere in the tree. Plain text does substring matching (`men` finds `X-Men`); a `*` acts as a wildcard (`star*war` finds both `Star Wars` and `Star Trek: The Mirror War`).

## Authentication

Set both `COMICSTREAM_AUTH_USERNAME` and `COMICSTREAM_AUTH_PASSWORD` to require HTTP Basic auth. Leave both unset for an open server. Most OPDS readers prompt for username and password when adding the server.

```yaml
COMICSTREAM_AUTH_USERNAME: alice
COMICSTREAM_AUTH_PASSWORD: changeme
```

HTTP Basic sends credentials on every request. On a LAN that's fine; if you ever expose ComicStream to the internet, put it behind a reverse proxy that handles TLS (Caddy, Traefik, nginx with Let's Encrypt).

## License

[AGPL-3.0-or-later](LICENSE).
