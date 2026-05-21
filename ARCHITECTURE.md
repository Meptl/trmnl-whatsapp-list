# Architecture

## Purpose

`trmnl-whatsapp-list` is a small Rust 2024 service for one shared list of text
entries. WhatsApp messages mutate or query the list, and a TRMNL device in BYOS
mode displays the current list.

The service intentionally avoids migrations, multi-list modeling, provider
fallbacks, and backward compatibility layers.

## Current State

The implemented foundation currently includes:

- Crate-level `#![forbid(unsafe_code)]`.
- Runtime configuration loaded from environment variables.
- Secret wrapper debug output that redacts token values.
- Initial dependencies for HTTP routing, SQLite, Meta Graph API calls, JSON
  payloads, TRMNL response types, and PNG rendering.

The executable currently loads configuration at startup and exits successfully
after configuration is validated. HTTP routing, persistence, command execution,
WhatsApp webhook handling, and TRMNL endpoints are planned but not yet wired.

## Configuration

Configuration is owned by `src/config.rs`.

Required environment variables:

- `WHATSAPP_VERIFY_TOKEN`
- `WHATSAPP_ACCESS_TOKEN`
- `WHATSAPP_PHONE_NUMBER_ID`
- `TRMNL_TOKEN`
- `PUBLIC_BASE_URL`

Optional environment variables:

- `DATABASE_PATH`, defaulting to `list.db`
- `BIND_ADDR`, defaulting to `127.0.0.1:3000`

Missing required variables return `ConfigError::MissingRequiredVariable` with
the variable name. Invalid Unicode in environment keys or values returns
`ConfigError::InvalidUnicode`. Secret values are stored in `SecretString`, whose
`Debug` implementation prints only `[redacted]`.

Tests use `AppConfig::from_pairs` so configuration behavior can be verified
without mutating process-global environment variables.

## Planned Runtime Shape

The service is expected to split into these responsibilities:

- Startup loads `AppConfig`, initializes persistence, builds the Axum router,
  binds `BIND_ADDR`, and serves requests.
- Persistence uses `rusqlite` directly against `DATABASE_PATH`.
- Command parsing and execution stay independent of WhatsApp payload shapes and
  HTTP handlers.
- WhatsApp integration targets the official Meta WhatsApp Cloud API only.
- TRMNL integration exposes BYOS display, PNG image, and telemetry endpoints.

## Planned Data Model

SQLite persistence will own one table named `entries` with:

- `id`
- `text`
- `created_at`

Listing order is creation order. Displayed numeric positions are 1-based and map
directly to that creation-ordered list.

## Planned HTTP Surface

The planned Axum routes are:

- `GET /webhooks/whatsapp`
- `POST /webhooks/whatsapp`
- `GET /api/display`
- `GET /trmnl/list.png`
- `POST /api/log`

TRMNL endpoints that expose display content require the shared `TRMNL_TOKEN`.
WhatsApp verification compares Meta's `hub.verify_token` against
`WHATSAPP_VERIFY_TOKEN` and returns the provided challenge only on a match.

## Verification Expectations

Before a coding bead is considered done, run:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features`
- `cargo nextest run`

If the local environment has a broken `RUSTC_WRAPPER`, clear it for verification
with `RUSTC_WRAPPER=`.
