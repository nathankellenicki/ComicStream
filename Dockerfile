# syntax=docker/dockerfile:1.7

FROM --platform=$BUILDPLATFORM rust:1-alpine AS builder

# musl-dev: libc headers; g++/make: needed by libunrar (C++) and libsqlite3-sys (C)
RUN apk add --no-cache musl-dev g++ make pkgconfig

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

# Dynamic-link to musl + libstdc++ so the runtime image is small and we don't
# have to ship a fully-static C++ toolchain.
ENV RUSTFLAGS="-C target-feature=-crt-static"

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
 && cp target/release/comicstream /usr/local/bin/comicstream


FROM alpine:3.20 AS runtime

RUN apk add --no-cache \
        ca-certificates \
        libstdc++ \
        libgcc \
        tini

COPY --from=builder /usr/local/bin/comicstream /usr/local/bin/comicstream

EXPOSE 8080

ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/comicstream"]
