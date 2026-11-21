# Public preview operations

Implementation plan: `../LAUNCH_PLAN.md`. Verification status and source identities: `../evidence/run-ledger.md`.

## Accounts and configuration

- DNS: Google Cloud project `peak-plasma-304519`, managed zone `hashcode`.
- UI: Vercel team `christophe-has-projects`, custom domain `hn.hashcode.dev`. Obtain the DNS target from the actual Vercel project/domain response.
- Session backend: `hn-session.hashcode.dev`, production VPS `38.242.207.222` (`tldw.link`); development is Netcup `62.83.128.195` (`dev.tldw.link`, SSH alias `tldw-dev`) since 2026-11-17, when the gateway, patched gVisor and publisher moved there from the retired OVH host `51.210.151.84` (see `tldw-infra/docs/NETCUP-SETUP.md`). On development, `tldw-infra` (`playbooks/tasks/accounts.yml`, `docker.yml`) provisions the `hnpreview` and `hnpublisher` accounts and the rootless daemon; its scope excludes production. Route source: `summarizer-compose/traefik/dynamic/hn-preview.yml`.
- Private R2 bucket: `hn-preview-snapshots`, account `eccf4de11ef0f3ba0b48357ca773f8a7`. The live launch uses the existing `dev/` channel on both hosts: one development publisher produces validated bundles and both gateways adopt them. `prod/` is reserved and unused. Treat `dev/` as the live shared feed: do not inject test bundles or move its latest pointer for experiments.
- Use a bucket-scoped object read/write credential for publishing and a separate object read-only credential for adoption. Wrangler's interactive OAuth is not the unattended service credential. No keys in static assets or visitor images.

Gateway configuration belongs to `~hnpreview/.config/hn-preview/gateway.env` (0600): `PORT=8782`, `PREVIEW_BIND=172.18.0.1`, `PREVIEW_HOST=hn-session.hashcode.dev`, `PREVIEW_ORIGIN=https://hn.hashcode.dev`, immutable `PREVIEW_IMAGE=sha256:…`, `DOCKER_RUNTIME=runsc-rootless`, and `SNAPSHOT=/home/hnpreview/demo/snapshot.json`. Private development uses loopback and the local origin instead. Production needs an inbound firewall rule restricted to its `traefik-public` bridge and subnet; binding to the bridge alone does not bypass the host firewall.

Adoption-only configuration belongs to `~hnpreview/.config/hn-preview/storage.env` (0600): `R2_ACCOUNT_ID`, `R2_BUCKET`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `PREVIEW_CHANNEL=dev|prod`. The gateway unit does not load this file. The account can read it, but visitor containers receive only the snapshot mount, never the home directory or service environment.

Publisher configuration belongs to `/etc/hn-preview/publisher.env` (root-owned 0600): the write credential variables above, publishing channel, and `ANTHROPIC_API_KEY`. Pin `{"provider":"anthropic"}` in `/var/lib/hn-publisher/config/hn/summary.json` (publisher-owned 0600). The existing native Fast profile then uses Anthropic directly; no OpenRouter key is installed. The separate `hnpublisher` account has no login, Docker/sudo membership or access to application secrets. It runs `hn` and `publish.mjs` from the signed release in `/var/lib/hn-publisher/releases/current` (see "Automatic server releases"). Do not enable its timer before a manual end-to-end publish/adopt passes.

## Rollout order

1. Validate scoped credentials with real dev bundle upload/readback and adoption. Keep the bucket private. Exercise invalid digest/schema and restart recovery before enabling automation.
2. Install user gateway/adoption units under `~hnpreview/.config/systemd/user` and publisher units under `/etc/systemd/system`. Enable hourly adoption and daily publishing after integration passes. Persistent timers catch up after downtime; manual reruns can incur additional inference calls.
3. Recheck the proxy bridge gateway. Bind only that private address; verify public port 8782 is closed. Visitors still have `network=none`. Other trusted host processes/bridge peers are not isolated from a host listener by its bind address alone.
4. Review and commit the canonical Compose change, then follow its normal dev-first deployment procedure. Verify certificate/WSS and unrelated app health before production. Never edit server Compose by hand.
5. Run `node preview/build-ui.mjs` from the CLI worktree. It builds public assets only in `preview/dist`, embeds `https://hn-session.hashcode.dev`, and generates matching CSP. Deploy that directory through the authenticated Vercel team, inspect its URL, attach `hn.hashcode.dev`, then apply the exact provider-requested DNS records. Never upload the repository root.
6. Test the real custom domain: automatic startup, Enter article/summary, page limit, wrong Origin/Host, session cap, disconnect/timeout, restart across a new snapshot, and provider failure retaining the last good version. Only then call launch complete.

## Recovery and limits

Preparation tries the first two eligible articles in each feed, preserving feed order. An empty, failed or under-40-word summary is discarded before trying the alternate. If neither produces a valid summary, the previous snapshot remains live. Publishing independently rejects any supplied summary under 40 words, then uploads and reads back the immutable bundle before changing `latest.json`. Rejection fails the scheduled run and keeps the published version; the local candidate remains a draft. The check catches obvious fragments, not factual errors, and makes no extra AI calls. Adoption validates hash/schema, preserves `<snapshot>.previous`, restarts the gateway and checks its digest. Failed restart restores the prior file and retries service startup; investigate if recovery also fails. Daily updates end active sessions; Restart fetches the new version.

Keep the prior pinned image and snapshot. For rollback, stop publishing/adoption timers, restore the prior gateway image/file and restart its user service. Remove only the preview route to take the demo down; do not disable the shared proxy or rootful applications.

No blanket bucket TTL is configured. Current/last-good data must survive publishing outages; old-object cleanup remains pending. Free-tier operation is not a billing guarantee; a run can make up to six summary calls. The patched gVisor binary remains a maintenance obligation until an official compatible release is validated.

## Live launch (2026-10-26)

Public UI: https://hn.hashcode.dev (Vercel project `hn-preview`, team `christophe-has-projects`). Gateway: https://hn-session.hashcode.dev on production38.242.207.222. The daily publisher ran on development 51.210.151.84 at launch and on Netcup 62.83.128.195 since 2026-11-17, with the approved Anthropic credential; production has only the bucket-scoped reader and hourly adoption. No production inference key or second daily generation schedule. The direct public gateway port8782 remains blocked.

The UI was deployed from `preview/dist` with the Vercel CLI. GitHub integration was unavailable. Since 2026-11-20 `.github/workflows/preview-ui.yml` rebuilds `preview/dist` and redeploys it to production when a change to the page's files reaches `main`; run it by hand from the Actions tab to redeploy without a change. Its `VERCEL_TOKEN` lives in the GitHub `production` environment, which only protected branches can use. The token is the Vercel access token "hn GitHub Actions (preview UI deploy)", scoped to the `hn-preview` project only, and it expires on 2027-11-20; after that the workflow fails until a new token replaces the secret. The workflow then checks that the live page still sends its Content-Security-Policy and points at the session gateway. Roll back from the Vercel dashboard by promoting the previous deployment. The gateway, visitor image and publisher binary are released automatically (see "Automatic server releases"); the page waits for the gateway it needs. Do not upload the repository root or generated credentials. Both host routes came from canonical infrastructure commit87c53e3. Public DNS, certificate, WebSocket/session limits, isolation settings and disconnect cleanup passed; browser evidence is in `evidence/launch/public-*`.

## Snapshot adoption Host check (2026-10-28)

The local readiness request must send `PREVIEW_HOST` while connecting to `PREVIEW_BIND`. Node fetch ignored the custom Host header, causing production to reject the probe with403 and roll back successful updates. Adoption uses `node:http.get` for this probe; strict gateway Host validation, digest verification and rollback remain intact. `node preview/publisher-check.mjs` exercises this with a real HTTP server requiring a hostname different from its loopback address.

## Feed item validation (2026-10-29)

Hacker News top/new feeds include job postings. The previous story-only snapshot validator rejected job50280824 and stopped the October29 capture before publication. Feed entries now accept stories and jobs, preserving their original order; comments and missing entries remain invalid. The regression uses that actual job and exercises the offline CLI feed reader. Deploy the updated native validator and visitor image to both gateways before running the updated publisher, because older readers reject bundles containing jobs. A failed capture retains the previous published snapshot; a healthy adoption job reporting `Snapshot unchanged` does not prove generation succeeded.

## Server release (2026-11-19 and 2026-11-20)

The gateway's visitor image and the native `/opt/hn-preview/hn` were released by hand from `main` at `db03844` (`hn 0.2.0`, the `h` fold key). Development switched on 2026-11-19 and production on 2026-11-20, after an explicit go.

| Item | Value |
|---|---|
| Image | `hn-preview:20261119-db03844`, ID `sha256:1f64317d717ee51271b265dc4c46a1143060ce19e9ee8cc9cc70fece7f6c6932` on both hosts |
| `image.tar` | sha256 `234fec9449f20a7954fcbea83cf348c50f133ef7066ee2a2750e8c2e9e57586d` |
| `/opt/hn-preview/hn` | sha256 `b16ecd882b49615ecdc3124cee1a126b4dc931cb34d8e3817c198768edd0f836`, copied out of the image so the publisher and visitors run the same build |
| Previous image pin | development `sha256:7297a971f98c173a4b73ea51f3833d3d95049200f7b684f2838aa4f0b1919126`, production `sha256:cd3da2a045fd8b43f98badbf1b4204e758f925382f005cf23f9cd301d4ebe6e7` |
| Rollback copies | `/opt/hn-preview/hn.previous-20261119` and `gateway.env.previous-20261119` on each host, removed on 2026-11-21 with the rest of this layout |

Checks that passed: the new `hn` validated each host's live snapshot; a snapshot captured by the new `hn` validated with development's old `hn`, production's old `hn`, and production's old visitor image under gVisor; a real session through each gateway showed the feed and the new help rows; the visitor container ran the new image with `runsc-rootless`, no network, a read-only filesystem and 256 MB, and was removed on disconnect; public port 8782 stayed closed. Development's scheduled publish at 03:00 UTC on 2026-11-20 was the first full run of the new `hn`: it captured 55 stories, prepared the three featured summaries and published snapshot `6ff3cfa2…`, which both hosts adopted at 03:01 UTC. Production's previous visitor image then served that snapshot until the 2026-11-20 switch.

### Manual server release (retired)

Until automatic releases were set up, these steps were run by hand. They are kept as a record: the layout they change (`/opt/hn-preview/hn`, `/opt/hn-preview/publisher`, the gateway code in `~hnpreview/demo` and `PREVIEW_IMAGE` in `gateway.env`) was removed on 2026-11-21, so they no longer apply as written. `~hnpreview/demo` now holds only the live snapshot and the gateway's working directory.

Snapshot readers go before the writer, and the gateway goes before a page change that alters their messages. Release development first and switch production only after development passes. The commands run over SSH as `shaance`; the one-off `sudo -n -u hnpreview` and root installs below are the exception the owner approved for this demo, not the TLDW routine-deploy path.

1. Build on development from a clean clone at the chosen commit: `docker build -f preview/Dockerfile -t hn-preview:<date>-<sha7> .`. Save it with `docker save`, copy `/usr/local/bin/hn` out of a container made from it, and record both checksums.
2. Check snapshot compatibility in both directions: the new `hn --validate-snapshot` on each host's live snapshot, and each old reader (host `hn` and current visitor image under `runsc-rootless`) on a snapshot from the new `hn --capture-snapshot`. Stop if any reader rejects it.
3. Copy `image.tar` and `hn` to each host and compare checksums.
4. On each host, confirm no visitor container is running and the adopter is idle, because the restart ends open sessions.
5. Load the image into the `hnpreview` rootless daemon (`DOCKER_HOST=unix:///run/user/<uid>/docker.sock`; the home directory is private, so load from a world-readable copy in `/tmp` and delete it after). Confirm the loaded ID matches the build.
6. Copy `/opt/hn-preview/hn` and `gateway.env` to `.previous-<date>`, install the new `hn` root-owned 0755 through a temporary name and `mv`, and set `PREVIEW_IMAGE=<loaded ID>` in `gateway.env` as `hnpreview`.
7. Restart `hn-preview.service` as `hnpreview`, check `/healthz`, then open a real session (production through `wss://hn-session.hashcode.dev/session` with origin `https://hn.hashcode.dev`) and inspect the visitor container's image, runtime, network, read-only flag and memory limit.
8. Delete the copied files and the rootful build tag. Keep the previous rootless image and `.previous` files until the next release.

To roll back, restore the two `.previous` files on that host and restart `hn-preview.service`; the previous image is still loaded.

## Automatic server releases (since 2026-11-21)

Design and review history: `../RELEASE_PLAN.md`. No person acts after a merge.

1. A merge that changes server code runs `.github/workflows/preview-server.yml`. It builds `preview/Dockerfile`, pushes `ghcr.io/shaance/hn-preview:<sha>`, signs the digest with cosign keyless and moves `:main` forward. The image carries the visitor `hn` and, under `/opt/release`, the gateway code, `publish.mjs` and `node_modules`.
2. On development, `hn-preview-release.timer` (as `hnpublisher`, every 15 minutes) runs `/opt/hn-preview/tools/rollout.mjs`. It is the only writer of `releases/rollout.json` in the snapshot bucket. It admits a newer signed `:main` once both readers run the good release, moves the stage `dev -> prod -> writer` as each reader reports the candidate, then records it as `good` and switches the publisher's own copy.
3. On both hosts, `hn-preview-update.timer` (as `hnpreview`, every 15 minutes) runs `/opt/hn-preview/tools/update.mjs`. It verifies, pulls and stages the release it should run, waits until no visitor is connected (at most two hours), switches `~/.local/lib/hn-preview/current` under the adopter's lock, then checks `/healthz`, a synthetic session showing a real story title and the visitor container's isolation. A failure returns the host to the good release and reports it, and the coordinator then aborts the rollout for every host.
4. The daily publish runs `/opt/hn-preview/tools/run.sh`. It first aligns the publisher with the record's good release, then uses that one build for capture, prepare and publish.
5. `.github/workflows/preview-monitor.yml` reads production's public `/healthz` hourly and keeps one `preview-alert` issue open while anything is wrong. `preview-ui.yml` deploys a page change only after production's good release contains the gateway it needs.

Where to look: production's `https://hn-session.hashcode.dev/healthz` shows `running`, `committed`, `failed`, `tools` and the rollout record. On a host, use `journalctl --user -u hn-preview-update` as `hnpreview`, and `journalctl -u hn-preview-release -u hn-preview-publish` for the coordinator and the publisher.

Changing the tooling itself (`preview/tools/*`, the unit files) is the one step that still needs a person: run `sudo preview/deploy/install-tools.sh dev|prod` from a checkout on each host. The monitor opens an issue while a host's tooling differs from `main`.

Scheduled workflows in a public repository are disabled after 60 days without repository activity, which would also stop the monitor.
