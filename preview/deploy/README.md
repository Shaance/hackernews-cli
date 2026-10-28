# Public preview operations

Implementation plan: `../LAUNCH_PLAN.md`. Verification status and source identities: `../evidence/run-ledger.md`.

## Accounts and configuration

- DNS: Google Cloud project `peak-plasma-304519`, managed zone `hashcode`.
- UI: Vercel team `christophe-has-projects`, custom domain `hn.hashcode.dev`. Obtain the DNS target from the actual Vercel project/domain response.
- Session backend: `hn-session.hashcode.dev`, production VPS `38.242.207.222`; development is `51.210.151.84`. Route source: `summarizer-compose/traefik/dynamic/hn-preview.yml`.
- Private R2 bucket: `hn-preview-snapshots`, account `eccf4de11ef0f3ba0b48357ca773f8a7`. The live launch uses the existing `dev/` channel on both hosts: one development publisher produces validated bundles and both gateways adopt them. `prod/` is reserved and unused. Treat `dev/` as the live shared feed: do not inject test bundles or move its latest pointer for experiments.
- Use a bucket-scoped object read/write credential for publishing and a separate object read-only credential for adoption. Wrangler's interactive OAuth is not the unattended service credential. No keys in static assets or visitor images.

Gateway configuration belongs to `~hnpreview/.config/hn-preview/gateway.env` (0600): `PORT=8782`, `PREVIEW_BIND=172.18.0.1`, `PREVIEW_HOST=hn-session.hashcode.dev`, `PREVIEW_ORIGIN=https://hn.hashcode.dev`, immutable `PREVIEW_IMAGE=sha256:…`, `DOCKER_RUNTIME=runsc-rootless`, and `SNAPSHOT=/home/hnpreview/demo/snapshot.json`. Private development uses loopback and the local origin instead. Production needs an inbound firewall rule restricted to its `traefik-public` bridge and subnet; binding to the bridge alone does not bypass the host firewall.

Adoption-only configuration belongs to `~hnpreview/.config/hn-preview/storage.env` (0600): `R2_ACCOUNT_ID`, `R2_BUCKET`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `PREVIEW_CHANNEL=dev|prod`. The gateway unit does not load this file. The account can read it, but visitor containers receive only the snapshot mount, never the home directory or service environment.

Publisher configuration belongs to `/etc/hn-preview/publisher.env` (root-owned 0600): the write credential variables above, publishing channel, and `ANTHROPIC_API_KEY`. Pin `{"provider":"anthropic"}` in `/var/lib/hn-publisher/config/hn/summary.json` (publisher-owned 0600). The existing native Fast profile then uses Anthropic directly; no OpenRouter key is installed. The separate `hnpublisher` account has no login, Docker/sudo membership or access to application secrets. Install its Node dependencies in `/opt/hn-preview/publisher` and tested native binary at `/opt/hn-preview/hn`. Do not enable its timer before a manual end-to-end publish/adopt passes.

## Rollout order

1. Validate scoped credentials with real dev bundle upload/readback and adoption. Keep the bucket private. Exercise invalid digest/schema and restart recovery before enabling automation.
2. Install user gateway/adoption units under `~hnpreview/.config/systemd/user` and publisher units under `/etc/systemd/system`. Enable hourly adoption and daily publishing after integration passes. Persistent timers catch up after downtime; manual reruns can incur additional inference calls.
3. Recheck the proxy bridge gateway. Bind only that private address; verify public port 8782 is closed. Visitors still have `network=none`. Other trusted host processes/bridge peers are not isolated from a host listener by its bind address alone.
4. Review and commit the canonical Compose change, then follow its normal dev-first deployment procedure. Verify certificate/WSS and unrelated app health before production. Never edit server Compose by hand.
5. Run `node preview/build-ui.mjs` from the CLI worktree. It builds public assets only in `preview/dist`, embeds `https://hn-session.hashcode.dev`, and generates matching CSP. Deploy that directory through the authenticated Vercel team, inspect its URL, attach `hn.hashcode.dev`, then apply the exact provider-requested DNS records. Never upload the repository root.
6. Test the real custom domain: automatic startup, Enter article/summary, page limit, wrong Origin/Host, session cap, disconnect/timeout, restart across a new snapshot, and provider failure retaining the last good version. Only then call launch complete.

## Recovery and limits

Publishing first rejects any supplied summary under 40 words, then uploads and reads back the immutable bundle before changing `latest.json`. Rejection fails the scheduled run and keeps the published version; the local candidate remains a draft. The check catches obvious fragments, not factual errors, and makes no extra AI calls. Adoption validates hash/schema, preserves `<snapshot>.previous`, restarts the gateway and checks its digest. Failed restart restores the prior file and retries service startup; investigate if recovery also fails. Daily updates end active sessions; Restart fetches the new version.

Keep the prior pinned image and snapshot. For rollback, stop publishing/adoption timers, restore the prior gateway image/file and restart its user service. Remove only the preview route to take the demo down; do not disable the shared proxy or rootful applications.

No blanket bucket TTL is configured. Current/last-good data must survive publishing outages; old-object cleanup remains pending. Free-tier operation is not a billing guarantee; a run can make up to three summary calls. The patched gVisor binary remains a maintenance obligation until an official compatible release is validated.

## Live launch (2026-10-26)

Public UI: https://hn.hashcode.dev (Vercel project `hn-preview`, team `christophe-has-projects`). Gateway: https://hn-session.hashcode.dev on production38.242.207.222. The daily publisher remains on development51.210.151.84 with the approved Anthropic credential; production has only the bucket-scoped reader and hourly adoption. No production inference key or second daily generation schedule. The direct public gateway port8782 remains blocked.

The UI was deployed from `preview/dist` with the Vercel CLI. GitHub integration was unavailable; updates currently require rebuilding that static directory and redeploying it with `--prod --scope christophe-has-projects`. Do not upload the repository root or generated credentials. Both host routes came from canonical infrastructure commit87c53e3. Public DNS, certificate, WebSocket/session limits, isolation settings and disconnect cleanup passed; browser evidence is in `evidence/launch/public-*`.

## Snapshot adoption Host check (2026-10-28)

The local readiness request must send `PREVIEW_HOST` while connecting to `PREVIEW_BIND`. Node fetch ignored the custom Host header, causing production to reject the probe with403 and roll back successful updates. Adoption uses `node:http.get` for this probe; strict gateway Host validation, digest verification and rollback remain intact. `node preview/publisher-check.mjs` exercises this with a real HTTP server requiring a hostname different from its loopback address.
