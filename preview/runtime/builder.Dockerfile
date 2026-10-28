FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git python3 build-essential crossbuild-essential-amd64 zip unzip clang pkg-config libffi-dev libssl-dev libbpf-dev libnuma-dev erofs-utils busybox-static patch && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://releases.bazel.build/8.8.1/release/bazel-8.8.1-linux-arm64 -o /usr/local/bin/bazel && chmod 755 /usr/local/bin/bazel
WORKDIR /work
ENTRYPOINT ["bazel"]
