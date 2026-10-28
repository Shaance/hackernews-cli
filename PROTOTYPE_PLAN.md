# Local preview experiment

> Historical experiment record. For the current public deployment and recovery procedure, see [preview/deploy/README.md](preview/deploy/README.md).

Scope: local Mac only. No VPS, cloud deployment, paid AI calls, or production credentials. Branch isolated from the main checkout. The first trial uses Docker Desktop's Linux VM; rootless/gVisor parity is a separate experiment and must not be inferred from baseline Docker results.

## Budget before edits

| Category | Changed-line budget | Artifacts |
|---|---:|---|
| Runtime and publisher | 1200 | snapshot schema/client, capture command, reader preload; local gateway, browser shell, Dockerfile |
| Verification | 350 | boundary test script plus browser journeys |
| Captured assets | separate generated data | one bounded live snapshot; no secrets |
| Documentation | 200 | plan and evidence report |
| Review corrections | 200 | only observed failures |

Existing homes: extend hn_client at its IO boundary instead of duplicating service/paging logic. A snapshot module owns the persisted bundle boundary. Extend the reader at initialization to preload existing result types. Publisher reuses reader source extraction. Gateway owns container/session lifetime and is separate from native CLI. Browser owns article presentation, CLI owns story selection; a bounded OSC event carries the selected story ID. The immutable snapshot is the source of truth for both.

New public surfaces: --snapshot PATH, --capture-snapshot PATH; schema version 1; local HTTP assets + /snapshot + /session WebSocket. Capture command is the real publishing entrypoint, not a test hook. No fake HN HTTP server or inference provider. No bundled credentials. Precomputed illustrative summaries must be labeled and must not be represented as paid-provider results.

Acceptance: real live feed capture, bounded comments with valid parent/child references, article extraction success/failure recorded; browse real CLI, categories, comments and captured article in browser; no network interfaces beyond loopback; readonly filesystem and no Docker socket; separate sessions; hard session cap and cleanup; last successful snapshot retained on failed capture; desktop and narrow-screen browser evidence. Tests target these boundaries, not internal mirroring.
