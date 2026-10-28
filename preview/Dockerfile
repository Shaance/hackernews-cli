FROM rust:1.90-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --release --locked && cp target/release/hn /hn
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /hn /usr/local/bin/hn
USER 65534:65534
ENV HOME=/tmp TERM=xterm-256color COLORTERM=truecolor HN_THEME=dark
ENTRYPOINT ["/usr/local/bin/hn"]
CMD ["--snapshot", "/data/snapshot.json"]
