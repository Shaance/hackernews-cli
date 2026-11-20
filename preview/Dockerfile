FROM rust:1.90-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --release --locked && cp target/release/hn /hn

FROM node:22-bookworm-slim AS release-deps
WORKDIR /release
COPY preview/package.json preview/package-lock.json ./
RUN npm ci --omit=dev

FROM debian:bookworm-slim
ARG COMMIT
ARG COMMIT_COUNT
LABEL org.opencontainers.image.source="https://github.com/Shaance/hn" org.opencontainers.image.revision="${COMMIT}" dev.hashcode.hn-preview.commit_count="${COMMIT_COUNT}"
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /hn /usr/local/bin/hn
COPY --from=release-deps /release/node_modules /opt/release/node_modules
COPY preview/server.mjs preview/docker.mjs preview/adopt.mjs preview/publish.mjs \
     preview/index.html preview/app.js preview/style.css preview/favicon.svg /opt/release/
USER 65534:65534
ENV HOME=/tmp TERM=xterm-256color COLORTERM=truecolor HN_THEME=dark
ENTRYPOINT ["/usr/local/bin/hn"]
CMD ["--snapshot", "/data/snapshot.json"]
