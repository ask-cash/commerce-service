# commerce-service

Payment infrastructure for Cash, backed by Stripe. Cash never talks to Stripe
directly: it calls this service's internal `/v1` API and receives signed events
back. Stripe webhooks are the source of truth for money state.

## Layout

Hexagonal: dependencies point inward, and the domain has no I/O.

| Crate | Role |
| --- | --- |
| `crates/domain` | `Money`, `Currency`, IDs, and (per feature) state machines. Pure. |
| `crates/app` | Use cases and the ports (traits) they depend on. `AppError`. |
| `crates/stripe-client` | Small typed Stripe client (pinned version, mandatory idempotency keys) and webhook signature verification. |
| `crates/store` | Postgres pool, migrations, readiness, repositories. |
| `crates/api` | axum routers: public (`/v1`, webhooks) and admin (`/healthz`, `/readyz`, `/metrics`); JWT auth; error mapping. |
| `crates/worker` | Interval job runner (webhook processing, outbox relay, reconciliation). |
| `crates/commerce` | The `commerce-service` binary: config, telemetry, `server` / `worker` / `migrate`. |
| `migrations/` | Forward-only SQL migrations, embedded in the binary. |
| `openapi/` | The contract Cash's client is generated from. |
| `deploy/helm/` | Chart: server + worker Deployments, pre-upgrade migrate Job, NetworkPolicy, webhook-only Ingress. |

## Ports

- `:8080` public router. `/v1/*` for Cash only (NetworkPolicy + JWT); `/webhooks/stripe` is the only path the Ingress exposes.
- `:9090` admin router. `/healthz` (liveness), `/readyz` (DB reachable and migrated; Stripe deliberately excluded), `/metrics`.

## Run locally

```sh
cp .env.example .env            # Stripe test key, Cash dev public key
docker compose up --build       # postgres -> migrate -> server + worker
curl localhost:9090/readyz
```

Or against a local Postgres without Docker for the app:

```sh
set -a; source .env; set +a
cargo run -- migrate
cargo run -- server             # and in another shell: cargo run -- worker
```

## Checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Configuration

Environment variables prefixed `COMMERCE_`, with `__` between sections (see
`.env.example`), optionally layered over a TOML file named by `COMMERCE_CONFIG`.
The service refuses to start with a live Stripe key outside production, or a
test key in production.

## Service-to-service auth

Cash signs a JWT per request (or caches one for under 10 minutes) with an
Ed25519 private key. This service holds only public keys, keyed by `kid`, so
rotation is: add the new public key here, switch Cash to the new private key,
then remove the old public key.
