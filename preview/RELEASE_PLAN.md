# Plan: automatic server releases for the browser trial (v3)

After a merge to `main` that changes what the servers run, no person acts. Development switches first. Production follows once development passes its checks. The daily publisher (the snapshot writer) switches only after both servers run the new build. A host that fails its checks returns to the last good build on its own. A stalled or failed release opens a GitHub issue.

## History

- **v1:** reviewed by the advisor and Astra (high): 3 blockers, 7 majors.
- **v2:** Astra marked 4 findings resolved and 6 partly resolved, and found 9 new ones; the advisor raised 4 more.
- **v3:** re-derived to remove the parts that caused most of them: self-updating tools, the rolling GitHub release, the writer fallback, and abort paths that left hosts on different builds.
- **v3 review:** Astra found 1 blocker and 6 majors, all folded in below; the advisor called v3 sound to build. Review stopped there by design. Remaining minor points are listed under residual risks.

## Owner rules

- CI never holds SSH or any credential to the servers.
- No sudo in routine deploys. Host setup is a one-time manual step.
- Snapshot readers switch before the writer. Development before production.
- No secrets read or printed. The `dev/` R2 prefix is the live feed.

## Measured state (2026-11-20)

| | Development (Netcup) | Production (Contabo) |
|---|---|---|
| Arch, `hnpreview` | x86_64, uid 1003, linger, rootless Docker 29.10.0 | x86_64, uid 1002, same |
| Gateway | `127.0.0.1:8782`, not public | `172.18.0.1:8782`, public `https://hn-session.hashcode.dev/healthz` (200 from outside) |
| Validator | root-owned `/opt/hn-preview/hn` (adopter + publisher) | same path, adopter only |
| Publisher | system unit as `hnpublisher`, R2 read/write + Anthropic key, 03:00 UTC | none |
| Gateway code | `~hnpreview/demo/*.mjs` + `node_modules`, hand-copied | same |
| Ownership | `tldw-infra` creates accounts and rootless Docker; units and `/opt/hn-preview` hand-installed | not managed |

Code facts the design relies on:
- `server.mjs:14` resolves `PREVIEW_IMAGE` to a local image ID at startup.
- `server.mjs:92` caps sessions at 3.
- `publish.mjs:16` validates through `HN_BIN`.
- `adopt.mjs:59` rejects snapshots over 16 MiB.
- Snapshots carry `generated_at`.
- `EnvironmentFile=` overrides `Environment=`.

## Two kinds of server code

| | Release content (changes often, automatic) | Release tooling (changes rarely, installed once) |
|---|---|---|
| What | `hn` binary (visitor + validator + writer), gateway code `server.mjs`, `docker.mjs`, `adopt.mjs`, `publish.mjs`, the gateway's page assets, production `node_modules` | `update.mjs` (reader updater), `rollout.mjs` (coordinator), `run.sh` (publish wrapper), systemd units, cosign, crane |
| Where on the host | `~hnpreview/.local/lib/hn-preview/<release>/`, `/var/lib/hn-publisher/releases/<release>/` | root-owned `/opt/hn-preview/tools/`, unit directories |
| How it changes | the signed image below | one setup run; the monitor opens an issue when `main`'s tooling differs from what production runs |

The tooling must stay outside the release. It is the part that recovers a bad release, so a bad release must not be able to replace it. This is the one remaining manual path, and it only applies to changes to the deploy system itself.

## The release: one signed image

CI (`.github/workflows/preview-server.yml`) runs on pushes to `main` that touch `src/**`, `Cargo.*`, `.dockerignore`, `preview/Dockerfile`, `preview/{server,docker,adopt,publish}.mjs`, `preview/package*.json` or the gateway page assets. It also runs on `workflow_dispatch`, one run at a time, so a newer run replaces a pending older one. `.dockerignore` allowlists exactly these inputs.

1. Checks out full history (`fetch-depth: 0`). `commit_count` is `git rev-list --count HEAD`, which a shallow clone would get wrong.
2. Builds `preview/Dockerfile`. The image gains `/opt/release/` with the gateway code, `publish.mjs`, the page assets and production `node_modules`. The binary stays at `/usr/local/bin/hn`.
3. Pushes `:<sha>`, signs that digest with cosign keyless (GitHub OIDC), annotated with `commit` and `commit_count`, and verifies the signature.
4. Moves `:main` to the digest only if its `commit_count` is higher than the one `:main` points at now. Rerunning an old workflow therefore can't move the channel backward.
5. If any step fails, the workflow opens or updates the `preview-alert` issue. A merged server change then never disappears silently.

**Release ID = registry digest.** It is immutable, kept forever by GHCR and covered by the signature. Every host verifies with `/opt/hn-preview/tools/cosign verify`, pinned to identity `https://github.com/Shaance/hn/.github/workflows/preview-server.yml@refs/heads/main` and the GitHub OIDC issuer.
- `hnpreview` pulls with its rootless Docker, then copies `/opt/release` and `hn` out of a stopped container.
- `hnpublisher` has no Docker. It extracts `hn` and `/opt/release`, which includes `publish.mjs` and its `node_modules`, with `crane export <digest>`.
- The local image ID used for `PREVIEW_IMAGE` is read from the host's own daemon after the pull.

## Rollout record

`releases/rollout.json` sits in the existing private bucket, outside `dev/`. Only `rollout.mjs` writes it, running as `hnpublisher` on development with the publisher's existing read/write key. Both readers read it with their existing read-only key.

```
{ "good":      {"digest", "commit", "commit_count"},     // whole fleet runs this
  "candidate": {"digest", "commit", "commit_count"} | null,
  "stage":     "dev" | "prod" | "writer" | "idle",
  "since":     "<iso>", "heartbeat": "<iso>", "last_abort": {"digest", "reason"} | null }
```

Each host's **desired release** follows from the record:
- **Development reader:** the candidate when the stage is `dev`, `prod` or `writer`; otherwise `good`.
- **Production reader:** the candidate when the stage is `prod` or `writer`; otherwise `good`.
- **Writer:** the candidate when the stage is `writer`; otherwise `good`.

A host that was offline reconciles to its desired release when it returns.

The coordinator runs every 15 minutes:

1. Write the heartbeat.
2. **`idle`:** first reconcile the local writer to `good` (below). Admit nothing until both readers report `running == committed == good`, which is what makes an abort finish. Then read `:main`'s digest and verify its signature. If its `commit_count` is greater than `good`'s and it isn't `last_abort`, set `candidate` and `stage=dev`.
3. **`dev`:** dev's loopback `/healthz` reports `committed == candidate`, so set `stage=prod`.
4. **`prod`:** production's public `/healthz` reports `committed == candidate`, so set `stage=writer`.
5. **`writer`:** all of it happens under the publish lock.
   1. Re-check that both readers report `committed == candidate`.
   2. Extract the candidate with `crane` and validate the live published snapshot from R2 with its `hn`.
   3. Write `good = candidate`, `candidate = null`, `stage=idle` to R2 and read it back.
   4. Only then flip `/var/lib/hn-publisher/releases/current`.
6. **Any stage before step 5.3:** a reader reports `failed == candidate`, or the writer validation fails. Set `last_abort`, `candidate = null`, `stage=idle`. Every host's desired release is then `good` again.

**Reconcile the local writer.** The R2 record is decided first and the local writer follows it. A crash between the two leaves the writer behind the record, never ahead of it. Under the publish lock, both the coordinator and `run.sh` compare `releases/current` with the record's `good` and flip it if it differs, before doing anything else. The 03:00 run therefore always publishes with the fleet's `good` build, and an abort can never follow a publish by a newer writer.

The writer switches only after both readers committed the candidate, and the rollout ends at that moment. So at every point each reader runs a build at least as new as the writer. Readers return to `good` only during an attempt, before the writer has moved. A reader therefore never meets a snapshot from a newer writer, without any extra compatibility check at publish time.

`hnpreview` can only report state. A compromised `hnpreview` could fake or withhold an acknowledgement. It never chooses bytes the publisher runs, because the publisher verifies the signed image itself.

## Reader updater (`/opt/hn-preview/tools/update.mjs`, both hosts, `hnpreview` user timer every 15 min)

Each release directory holds `hn`, the gateway code, `node_modules` and `release.env`, which sets `PREVIEW_IMAGE=<local image ID>` and `RELEASE=<digest>`. `current` is a symlink. Host settings stay in `gateway.env`, untouched. The snapshot stays at `~/demo/snapshot.json`, and `~/demo` stays the working directory.
- The gateway unit reads `current/release.env` after `gateway.env` and runs `current/server.mjs`.
- The adopter unit runs `flock <lock> node current/adopt.mjs` with `HN_BIN=%h/.local/lib/hn-preview/current/hn`. Both the script and the validator resolve through `current` only after the lock is held.

The rollback target `known_good` is always the record's `good`. Pruning never removes `good`'s directory or image.

1. **Recover.** If `attempt.json {target, known_good}` exists, the last run stopped mid-switch.
   - Re-read the rollout record under the lock.
   - If the target is still desired, explicitly start it and run the checks. Otherwise, or if the checks fail, start `known_good` and check it.
   - Finish through the same pass or fail writes as step 7, then delete `attempt.json`.
2. Read the rollout record. If the desired release equals `current`, stop. A no-op may refresh `committed.json` only when `running`, `current` and `committed` all agree; otherwise run the step 6 checks first.
3. **Stage, without the lock:** verify the signature, pull by digest, read the local image ID, copy the files out.
   - Network, registry and Sigstore failures retry on the next tick and change nothing.
   - A signature that doesn't verify for a digest the record names is reported as `failed`.
4. If any visitor container is running, try again next tick. After 2 hours of waiting, switch anyway. Sessions last at most 10 minutes.
5. **Under the lock:**
   - Re-read the rollout record and stop if the desired release changed.
   - Validate the local snapshot with the new `hn`. A rejection is reported as `failed`.
   - Write `attempt.json`, flip `current`, restart `hn-preview.service`.
6. **Checks.** All must pass, with up to 5 minutes for session retries.
   - `/healthz` reports the live snapshot and `running == target`, which the gateway reads from `RELEASE` at startup.
   - A synthetic session with the configured Origin and Host renders the feed. A 503 when sessions are full is retried.
   - The session's container used the target's local image ID with `runsc-rootless`, no network, a read-only filesystem and 256 MB, and was removed afterwards.
7. **Pass:** write `committed.json {committed: target}` and delete `attempt.json`.
   **Fail:** flip back, restart, check `known_good`, then write `committed.json {committed: known_good, failed: target, reason}` and delete `attempt.json`.
   A release reported `failed` is not retried on that host. Only a new candidate changes that.
8. Keep `current`, `good` and the previous release directory and image; prune the rest.

The gateway's `/healthz` reads `committed.json` on each request and returns these fields:
- `snapshot`, `generated_at`, `running`, `committed`, `failed`;
- `tools`, the tooling version;
- on production, also the rollout stage, candidate, `good` (digest and commit), `last_abort`, `since` and `heartbeat`, copied from the record by the updater.

## Writer (development)

- `hn-preview-publish.service` becomes `ExecStart=flock /var/lib/hn-publisher/publish.lock /opt/hn-preview/tools/run.sh`.
  - `run.sh` resolves `releases/current/hn` once and runs capture, prepare and `publish.mjs` with that path as `HN_BIN`.
  - The coordinator's writer switch takes the same lock.
- `publish.mjs` gains the adopter's 16 MiB limit before upload, so it never publishes a snapshot both adopters would drop.
- A publish that fails before it moves `dev/latest.json` keeps yesterday's snapshot live, as today, and the monitor's stale-snapshot check reports it. One that fails at the readback after the move may already be live. Before anyone reruns capture (which costs inference), compare the remote pointer with the candidate hash. There is no automatic writer rollback. A writer bug shows up as a stale feed and is fixed by the next release.

## Page order

`preview-ui.yml` becomes a convergence job that runs on pushes and hourly. It uses one checkout SHA for everything. The built page embeds its commit, and the job deploys only when both hold:
- The live page's commit differs from that checkout's page commit.
- Production's `/healthz` reports `running == committed == good.digest`. The newest commit that changed the gateway (`server.mjs`, `docker.mjs`) at or before the checkout must be contained in `good.commit`, checked with the GitHub compare API.

Gating on the fleet's `good` release, not on production's latest acknowledgement, means a page goes out only after its gateway can no longer be rolled back by an abort.

A page never reaches visitors before the gateway it needs. No job polls for hours, and a delayed page goes out on the first hourly run after production commits.

## Monitor (`preview-monitor.yml`, hourly, no server credentials)

It reads production's public `/healthz` and `:main`'s digest anonymously from GHCR. It opens or updates one issue labelled `preview-alert` while any check fails, and closes it when all pass. GitHub notifies on the opened issue.

| Check | Fails when |
|---|---|
| Rollout moving | stage `dev` or `prod` for more than 6 h; `writer` for more than 1 h |
| Coordinator alive | heartbeat older than 1 h |
| Release reached production | `:main` signed digest has a higher `commit_count` than `good`, with stage `idle` and no `last_abort` for that digest, for more than 1 h |
| Hosts on `good` | stage `idle` and production not `running == committed == good` for more than 1 h |
| CI release | the server workflow failed (it opens the issue itself; the monitor clears it only when a signed release covers that commit) |
| Abort | `last_abort` set for `:main`'s digest |
| Feed fresh | `generated_at` older than 30 h |
| Tooling current | `tools` differs from `main`'s tooling version |
| Endpoint | `/healthz` unreachable or missing fields (after it first reported `committed`) |

Scheduled workflows in a public repository are disabled after 60 days without repository activity, so the README records this as a known limit. During the first release I will trigger one alert on purpose to confirm the issue notification arrives.

## One-time host setup (sudo, owner go, development first)

The starting `good` release is today's build, `db03844`. It has no registry digest, so its record uses `{"digest": "bootstrap-db03844", "commit": "db03844…", "commit_count": <count>}`. On each host its release directory is seeded from what runs now:
- the loaded image ID `1f64317d…`;
- `/opt/hn-preview/hn`;
- today's `~/demo` gateway files, plus the new `server.mjs` from PR A so `/healthz` reports `committed`;
- `publish.mjs` and its `node_modules` for the publisher.

The new code (current `main`, with the 0.2.2 "1w" fix) then arrives through the normal automatic path, not through setup.

1. **Both hosts:**
   - install cosign and crane (pinned versions with checksums) and the tools in `/opt/hn-preview/tools/`;
   - install the new user units;
   - seed the bootstrap release directory, `current`, and `committed.json {committed: "bootstrap-db03844"}`;
   - restart the gateway once and check it.
2. **Development:**
   - install the publisher units;
   - seed `/var/lib/hn-publisher/releases/bootstrap-db03844/` and its `current` symlink;
   - write `rollout.json` with `good` = bootstrap and `stage=idle`;
   - only then enable the timers.
3. **GitHub (once):** make the GHCR package public.
4. **Old layout:** `/opt/hn-preview/hn`, `/opt/hn-preview/publisher` and the old code files in `~/demo` are removed only after the first automatic release has gone back to `idle` with `good` updated, and one real adopter run has used the new path. The snapshot and the `~/demo` working directory stay.

## Landing units

1. **PR A, no host effect:**
   - `preview-server.yml`;
   - the Dockerfile `/opt/release` stage;
   - the `/healthz` fields in `server.mjs`;
   - `publish.mjs` size check.

   Verification: a dispatch builds and signs, the image contains every file the hosts extract, and the digest is a single manifest or an index that all three tools accept.

   **Host primitive check, before writing PR B.** On development, under the real accounts:
   - `hnpreview`: rootless `docker pull` by digest, and `cosign verify` (needs Sigstore egress and a writable cache).
   - `hnpublisher`: `cosign verify` and `crane export` inside a transient unit with the publisher's sandbox (`ProtectSystem=strict`, 512 MB, 64 tasks), plus the loopback `/healthz` read.

   These run under `sudo` once, as part of setup. If any of them fails, the design changes before PR B.
2. **PR B, tooling:**
   - `update.mjs`, `rollout.mjs`, `run.sh`, the adopter lock, the units;
   - offline tests (fake R2 and registry server, stub Docker socket, local gateway). They cover:
     - apply and no-op;
     - signature failure;
     - snapshot rejection;
     - health, session and container checks failing, with rollback;
     - recovery from `attempt.json` before and after the flip;
     - the rollout record changing between staging and switch;
     - abort sending all hosts back to `good`;
     - a returning host reconciling;
     - `commit_count` blocking a backward move;
     - writer switch under the publish lock;
   - README replaces the manual runbook.
3. **PR C, page convergence and monitor:** both are no-ops until production's `/healthz` reports `committed`.
4. **Host setup**, development then production. Each tool first runs with `--check`, which logs what it would do.
5. **First automatic release:** the CI release from step 1 becomes the candidate on its own. Watch these, then check the "1w" ages on the public page:
   - dev commits;
   - stage moves to `prod`, and production commits;
   - the writer switches and the stage returns to `idle`;
   - the 03:00 UTC publish runs and both hosts adopt it;
   - the monitor stays green.

## Residual risks

- Sigstore, GHCR or GitHub outages delay a release and fail closed. The monitor reports the delay after 6 h.
- Tooling changes need one setup run. The monitor opens an issue when they are pending.
- A restart ends any session started after the idle check, which lasts at most 10 minutes.
- A compromised `hnpreview` can delay or falsely acknowledge a rollout. It cannot make the publisher run unsigned code.
- The monitor stops if GitHub disables scheduled workflows after 60 quiet days.
