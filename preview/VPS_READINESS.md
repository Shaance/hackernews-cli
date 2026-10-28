# VPS validation, 26 October 2026

> Historical experiment record. For the current public deployment and recovery procedure, see [deploy/README.md](deploy/README.md).

The isolated preview runs on both existing Debian VPSs. Native CLI sessions, resource limits, private HTTPS/WSS, origin checks and cleanup passed. Public deployment is still pending.

| Check | Development VPS | Production VPS |
|---|---|---|
| Dedicated identity | `hnpreview`, UID 1003, own group only, nologin | `hnpreview`, UID 1002, own group only, nologin |
| Separate Docker daemon | Rootless, own Unix socket | Rootless, own Unix socket |
| Official gVisor 20261019.0 | Container creation fails with systemd authentication error | Container creation fails with systemd permission error |
| Patched gVisor | Native AMD64 runtime passes | Native AMD64 runtime passes |
| CPU, memory, process pressure | Limits enforced in host cgroups | Limits enforced in host cgroups |
| CLI and admission | Three sessions work; fourth returns 503 | Three sessions work; fourth returns 503 |
| HTTPS/WSS and origins | Passed through private loopback TLS proxy | Passed through private loopback TLS proxy |
| Lifetime and recovery | Timeout, disconnect, quit, startup disconnect, crash/restart cleanup pass | Same checks pass |
| Snapshot adoption | Pinned until restart; replacement adopted after restart | Same behavior |
| Existing applications | Six still running | Nine still running |

The preview account cannot read the existing Docker socket, known application env files, deployment registry credentials, SSH directory or ACME certificate file. Existing application data volumes are inaccessible. The certificate directory and Prometheus configuration are readable; the certificate file itself is root-only. These are scoped permission checks, not an exhaustive secret inventory or a penetration test.

Each visitor runs as UID 65534 with no capabilities, no network, a read-only root filesystem, one read-only snapshot mount and a 16 MiB temporary filesystem. Its limits are 256 MiB memory, half a CPU and 64 host tasks. The whole preview account, including its daemon, is additionally capped at 768 MiB, one CPU and 256 tasks. Those aggregate caps deliberately limit total host impact; they do not reserve each visitor's maximum allocation simultaneously.

The host cgroups showed CPU throttling, process-limit events and memory OOM events. Excessive process or memory pressure can terminate the gVisor sandbox. This is bounded failure, not graceful handling inside the CLI. Each probe removed its container and cgroup afterward. The gateway separately passed disconnect, quit and lifetime cleanup; forced gateway death left an orphan that its next startup removed.

The production sample took 713 ms to report the session ready and 2,572 ms to render the first feed, with about 29 MiB idle container memory. These are single measurements, not cold-host benchmarks.

## Runtime and source identity

The custom runtime uses the complete backport documented in [GVISOR_FIX.md](GVISOR_FIX.md). Both hosts use the complete AMD64 bundle, including the required sidecars. It was built on the Mac with `--config=x86_64`; the official bundle was checksum-verified before installation. The custom build reports version `0.0.0`, so the source patch and artifact digest identify it.

- Custom bundle SHA-256: `6f29229277c2a5e26cb36ee4467149283eb85daa2f0c0aeb7d8a57b1ee163768`.
- CLI image config ID: `sha256:4d4cd5d588285c56445591a8e7ed047e740aa1009790a61bf274bef14457dfb3`, AMD64.
- Host runtime directories: `/opt/hn-preview/gvisor-patched`, `/opt/hn-preview/gvisor-official`, `/opt/hn-preview/node`.
- Frozen gateway copy: `/home/hnpreview/demo`; worktree edits do not automatically update it.
- Rootless sockets: `/run/user/1003/docker.sock` on development and `/run/user/1002/docker.sock` on production.

`setup-rootless.sh` provisions a new dedicated identity and refuses to run over an existing account. It was exercised once on each host. The existing Compose configuration, rootful Docker daemon, DNS and proxy routes were not changed. Existing application start times predate this validation; no new listening ports remain. The dedicated rootless daemons remain available, with zero preview containers and no gateway running.

## What remains before public launch

1. **Wire the real UI and backend domains.** `PREVIEW_ORIGIN` accepts exactly the frontend origin, and `PREVIEW_HOST` accepts exactly the backend Host. Private tests used `https://hn.hashcode.dev` and `hn-session.hashcode.dev`. The current UI still connects to same-origin relative endpoints; a split Vercel/backend deployment needs its endpoint and CSP configuration. No Vercel deployment, public DNS change, public certificate or real Traefik WebSocket route was tested here.
2. **Supervise the gateway.** Crash recovery was exercised by restarting it from the harness. A permanent service and its restart policy still need deployment. Origin checks protect browser requests; they do not authenticate non-browser clients or prevent someone repeatedly occupying the three available slots.
3. **Choose the snapshot update policy.** A running gateway pins its bytes even if the source file changes. Restart adopts the new file and ends current sessions. Invalid JSON refuses startup, and restoring the last validated file recovers service. R2 download/publish, cron and seamless version adoption are not implemented. Use the existing publisher's validation before replacing a bundle.
4. **Own the custom runtime.** The official release still fails in this configuration. Retain the exact patch/build evidence and retest an official release containing the fix before replacing this build. No independent security review of the backport is claimed.

## Evidence and reproduction

The canonical run ledger is `preview/evidence/run-ledger.md`. Raw evidence lives in `preview/evidence/vps-readiness/`: account/mount permission checks, official and patched runtime results, boundary tests, private TLS/snapshot tests, lifecycle tests, resource pressure logs, and final source/image/application readback. The final report hashes matched the local gateway and test sources on both hosts.

Run `verify.mjs` against a running gateway, `lifecycle.mjs` for a separate crash/timeout gateway, and `gvisor_probe.py pids` then `gvisor_probe.py memory` on the Linux host under the preview identity. Set `DOCKER_SOCKET`, `DOCKER_HOST`, `PREVIEW_IMAGE=hn-preview:vps-check` and `DOCKER_RUNTIME=runsc-rootless` for that account. The pressure probe reads real host cgroups because gVisor virtualizes its guest `/sys`.

`public-check.mjs` starts its own loopback gateway and TLS proxy. It requires a short-lived test certificate with `DNS:localhost,IP:127.0.0.1`, supplied through `TEST_TLS_CERT` and `TEST_TLS_KEY`; the client explicitly trusts that certificate and verifies its hostname. Test certificates were removed after use. It asserts allowed-origin success, foreign-origin rejection, wrong-Host rejection, real CLI article selection, disconnect cleanup and snapshot restart adoption. A mutation check confirmed removing the new Host/Origin settings causes the HTTPS success assertion to fail.
