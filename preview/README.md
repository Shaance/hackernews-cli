# Browser preview

Try the native CLI at [hn.hashcode.dev](https://hn.hashcode.dev). The static UI is hosted on Vercel; a dedicated VPS account runs the gateway and an isolated container for each visitor. A separate publisher generates daily snapshots and uploads them to private R2 storage. See [deployment operations](deploy/README.md) for schedules, accounts and recovery.

## Behavior and boundaries

- Top, New and Best contain up to 20 stories each and up to 20 captured comments per story.
- A fresh session selects a useful captured summary without changing feed ranking. X/Twitter stories remain selectable but are excluded from automatic selection.
- Enter opens the summary/comments in the real CLI and the original website in a browser iframe. Sites can block embedding; Open original remains available.
- Visitor containers have no network, a read-only filesystem, temporary session storage, and CPU/memory/process limits. They receive no Docker socket or provider/storage credentials.
- Three sessions can run concurrently, for ten minutes each. Quit, disconnect and expiry remove the container. Saved stories last only for that session.
- The online publisher reuses the native article extractor and Anthropic summary provider. It makes at most six summary calls per run (two eligible candidates per feed). Publishing validates content before advancing the latest pointer; adoption checks the digest/schema and restores the previous snapshot if readiness fails.

## Reproduce from a clean checkout

Prerequisites: Rust 1.88 or newer, Cargo, and Node 22 or newer. Run these commands from the repository root:

```sh
cargo build --locked
cargo test --locked
npm ci --prefix preview
npm test --prefix preview
npm run format:check --prefix preview
npm run build --prefix preview
```

The publisher/adoption tests use `fixtures/snapshot.json`, a small synthetic fixture with an illustrative summary and comment. They need no live capture, API key, R2 account, Docker or paid inference. They run the real native validator and an actual HTTP readiness endpoint, with R2 and systemctl substituted at process boundaries. `HN_BIN` can override the default `target/debug/hn` binary. They cover failed publication, short summaries, schema/digest errors, failed readiness/rollback and unchanged-version adoption. `npm run test:sensitivity --prefix preview` temporarily removes the summary floor and readiness error detail in disposable copies and verifies that the regression checks fail. The format command is `npm run format --prefix preview`.

## Local browser and sandbox checks

With a dedicated Docker daemon available (ordinary Docker suffices for a local baseline; it does not establish rootless/gVisor isolation):

```sh
docker build -f preview/Dockerfile -t hn-preview:local .
SNAPSHOT=fixtures/snapshot.json node preview/server.mjs
```

Open `http://127.0.0.1:8765/`. The fixture is synthetic, so its article links are example domains. To use a current capture instead:

```sh
cargo run --locked -- --capture-snapshot preview/snapshot.json
# Optional, uses your configured provider and can incur charges:
cargo run --locked -- --prepare-snapshot preview/snapshot.json
node preview/server.mjs
```

Capturing contacts public HN/article sources; it does not call an AI provider. The gateway pins the startup snapshot. Restart it to load a changed file. While the fixture-backed gateway is running:

```sh
npm run test:sandbox --prefix preview
npm run test:lifecycle --prefix preview
```

The lifecycle check starts its own gateway on 8768. `test:proxy` additionally requires `TEST_TLS_CERT` and `TEST_TLS_KEY` for a trusted local test certificate with localhost SAN, and uses 8770/8771. These integration checks require the test image/daemon and must not target the production gateway. Evidence directories are created automatically and ignored by Git.

`DOCKER_SOCKET` chooses a daemon; `DOCKER_RUNTIME` chooses an installed runtime. `PREVIEW_IMAGE`, `PORT`, `PREVIEW_BIND`, `PREVIEW_HOST` and `PREVIEW_ORIGIN` configure the gateway. Defaults serve localhost only. Do not run two gateways sharing a daemon and port identity: startup cleanup owns containers with that label.

## Source ownership and deployment

| Source | Responsibility |
| --- | --- |
| `src/snapshot.rs`, `src/snapshot_capture.rs` | Versioned input, native validation and bounded capture |
| `src/reader/snapshot.rs` | Captured summary loading and publisher enrichment |
| `publish.mjs`, `adopt.mjs` | R2 publication and validated local adoption |
| `server.mjs`, `docker.mjs` | Gateway/session lifetime and Docker transport |
| `app.js`, `index.html`, `style.css`, `favicon.svg` | Browser terminal, article pane and page shell |
| `build-ui.mjs` | Static deployment allowlist and browser security headers |
| `deploy/` | Service/timer definitions and operator instructions |
| `runtime/` | Source patch and compiler recipe for the deployed custom gVisor build |

The UI build outputs only public assets to `preview/dist`; deploy that directory, never the repository root. Live snapshots, credentials, build output, binaries and local evidence are excluded from Git. A clean checkout reproduces tests and UI builds; provisioning the hosted runtime still requires the credentials and reviewed host configuration documented in `deploy/README.md`.

The gVisor backport remains a maintenance obligation. [GVISOR_FIX.md](GVISOR_FIX.md) and [VPS_READINESS.md](VPS_READINESS.md) are dated experiment records, not claims about future runtime releases. The repository includes the adapted source patch and ARM builder used for the experiments; no runtime binary is checked in. Read the recorded architecture/build constraints before rebuilding. No independent penetration test is claimed.
