# Recorded gVisor build inputs

These are the unchanged source inputs used for the preview runtime described in [the repair report](../GVISOR_FIX.md). This directory is not an automatically installed runtime or a second application implementation.

- Base: google/gvisor tag `release-20261019.0`.
- Upstream rootless systemd changes: [PR 13853](https://github.com/google/gvisor/pull/13853), head `8ecc635e5e32b8eb1cb2acdc308d480f00bf7264` at the time of the experiment.
- `complete-backport.patch` includes the adapted cgroup/systemd changes, their tests and the Nogo build correction. Apply with `patch -p1` from the base source root.
- `builder.Dockerfile` downloads the ARM64 Bazel 8.8.1 executable and is intended for the Mac experiment's ARM64 Linux build host. The original AMD64 runtime was cross-built there using `--config=x86_64`; the ARM runtime used `--config=aarch64`.
- Build target: `//debian:gvisor-release-tar-bz2`, with `-c opt --jobs=3 --spawn_strategy=local`.

The upstream project is [Apache 2.0 licensed](https://github.com/google/gvisor/blob/release-20261019.0/LICENSE); preserve its license/notices when distributing a complete source tree or binary. Build output, caches and installed credentials do not belong in this repository. Future runtime upgrades require fresh kernel/resource-limit verification; retaining these inputs does not certify an arbitrary new build. This handoff did not rebuild gVisor.
