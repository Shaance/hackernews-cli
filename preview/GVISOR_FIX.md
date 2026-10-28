# Rootless gVisor repair, 26 October 2026

> Historical experiment record. For the current public deployment and recovery procedure, see [deploy/README.md](deploy/README.md).

The local prototype now runs through **rootless Docker plus gVisor**, with host-enforced resource limits intact. Open http://127.0.0.1:8767/ on this Mac. The original rootless/runc demo remains on port 8766. Nothing was changed on the VPS.

## Cause and repair

The installed `release-20261019.0` runtime always connected to the system-wide systemd manager for cgroups. The rootless account cannot create system-wide scopes. The original limited container still reproduced `systemd error: Permission denied` before modification.

[Upstream issue 11543](https://github.com/google/gvisor/issues/11543) matches this failure. [Open PR 13853](https://github.com/google/gvisor/pull/13853), head `8ecc635e5e32b8eb1cb2acdc308d480f00bf7264`, connects to the user manager, authenticates using the UID outside the user namespace, and places resource controls under that manager's delegated hierarchy. Its second commit checks that requested controllers exist. The PR was unmerged when checked.

I reviewed and backported both commits to the installed release. Two merge conflicts were resolved by preserving the release's newer IO-limit application and test import. The VM already delegated CPU, memory, IO and PID controllers; no additional permissions or system-manager authorization were needed.

Building also exposed an unrelated mismatch between Nogo's source list and the Go SDK's compiled standard library. The recorded build patch includes the complete non-test source inventory and respects the analyzer's existing no-Cgo contract when selecting parsed files. Static analysis remained enabled. These are experimental source changes, not an official gVisor release.

## What passed

| Check | Evidence |
|---|---|
| Rootless start | Same offline/read-only/capability-free container now starts and reports `4.19.0-gvisor` |
| Correct authority | Scope loaded by user systemd; not found in system systemd; path beneath `user@1000.service` |
| Real host limits | `memory.max=268435456`, `cpu.max=50000 100000`, `pids.max=64` |
| Network/filesystem isolation | Only loopback; public/metadata/bridge TCP and DNS blocked; UID 65534; zero effective capabilities; no-new-privileges; root filesystem writes refused |
| CPU pressure | Host `nr_throttled` increased under a bounded busy loop |
| PID pressure | Host `pids.events:max` increased; sandbox terminated; cgroup removed |
| Memory pressure | Host `memory.events:oom_kill` increased and Docker recorded OOMKilled; cgroup removed |
| Native browser journey | Real CLI, featured article, curated summary toggle, comments in Chrome; live container inspected as runtime `runsc-rootless` |
| Lifecycle | Timeout, forced gateway crash, orphan recovery, graceful shutdown, startup disconnect all passed |
| Runtime checks | 42 top-level cgroup unit tests passed; cgroup static-analysis target passed |

One warm browser start took 244 ms until gateway ready. An idle sample was 17.75 MiB and 29 host tasks. These are samples, not performance guarantees.

The PID stress case exposes an availability limitation: gVisor's systrap runtime can panic when the kernel refuses another process/thread. The host limit holds and the affected session ends; it does not gracefully return a fork error in every case. This is acceptable evidence of containment, not a claim that gVisor handles exhaustion cleanly. The gateway remained available.

Test corrections are preserved: the first pressure checker read counters after runtime cleanup had removed them, so it now observes them live and runs destructive scenarios in fresh containers. A slow shell-string allocation timed out without reaching the memory limit; a bounded native `sort` allocation reached it. The first unit run had root filesystem-bypass capabilities, invalidating an existing permission-denial test; the same tests passed with all capabilities dropped. No assertions were removed or relaxed to obtain a pass.

## Reproduce and operate

The patched bundle is installed only at `/opt/hn-preview/gvisor-rootless/` inside the dedicated VM. Packaged `/usr/bin/runsc` is unchanged. Only the `hnpreview` account's Docker configuration gained a separate `runsc-rootless` runtime with `--platform=systrap`. Debug logging used during diagnosis is now disabled. No `--ignore-cgroups`, privileged visitor container, or system-wide permission bypass was used.

```sh
# Launch the patched browser gateway if port 8767 is not already serving it.
colima ssh --profile hn-preview -- sudo -u hnpreview env \
  DOCKER_SOCKET=/run/user/1000/docker.sock DOCKER_RUNTIME=runsc-rootless PORT=8767 \
  node /home/hnpreview/demo/server.mjs
# Run each destructive limit probe in a fresh container inside this VM.
colima ssh --profile hn-preview -- sudo -u hnpreview env \
  XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  python3 /Users/shaance/Documents/projects/hn-preview-prototype/preview/gvisor_probe.py pids
# Repeat the preceding command with 'memory' in place of 'pids'.
```

`runtime/complete-backport.patch` and `runtime/builder.Dockerfile` retain the complete adapted patch and ARM compiler recipe. Build/test logs, screenshots and runtime archives remain local ignored evidence. Source archive SHA-256: `fbae7b0693134fcb435683c482b0864877a433d6f7630ca3e3d33a43fa4a42d7`; compiled bundle: `e0156dbf935974f30fa77b6e1941cdc6975641c82866dc931f7ea74b781233e9`. The binary reports version `0.0.0` because it was built from a source archive; use the bundle hash as its identity.

To rebuild, unpack upstream tag `release-20261019.0`, apply `complete-backport.patch`, and use `runtime/builder.Dockerfile` (ARM64 build host) with Bazel 8.8.1: `bazel build -c opt --config=aarch64 --jobs=3 --spawn_strategy=local //debian:gvisor-release-tar-bz2`. The compiler mounts only source and its build cache, without host credentials or Docker socket. The initial runtime verification was Linux ARM64 in the Mac's dedicated VM. The complete bundle has since been built with `--config=x86_64` and tested on both AMD64 VPSs; see [VPS_READINESS.md](VPS_READINESS.md).

For rollback, stop the gateway using `runsc-rootless` and use the port-8766 baseline. The prior daemon configuration is saved as `/home/hnpreview/.config/docker/daemon.before-gvisor-fix.json`. Stopping `colima stop hn-preview` stops both local demos. Before public deployment, choose an official fixed runtime release or explicitly maintain and review the custom build, then repeat the checks on the target kernel.
