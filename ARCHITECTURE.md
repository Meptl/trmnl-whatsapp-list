# Architecture

## Purpose

`trmnl-whatsapp-list` is a small Rust 2024 service for one shared list of text
entries. It is designed so WhatsApp messages toggle list entries, and a TRMNL
device in BYOS mode displays the current list.

The service intentionally avoids migrations, multi-list modeling, provider
fallbacks, and backward compatibility layers.

## Current State

The implemented foundation currently includes:

- Crate-level `#![forbid(unsafe_code)]`.
- Runtime configuration loaded from environment variables.
- Secret wrapper debug output that redacts token values.
- Axum startup that loads configuration, initializes application state, binds
  `BIND_ADDR`, and serves requests.
- Router for WhatsApp and TRMNL endpoints.
- WhatsApp verification for `GET /webhooks/whatsapp`.
- SQLite persistence initialization and list operations.
- Message text toggling for add/remove behavior, slash commands for list and
  clear, and ignored empty input.
- WhatsApp Cloud API webhook payload parsing for inbound text messages.
- Meta Graph API text reply request construction and sending through the
  WhatsApp webhook POST flow.
- TRMNL BYOS display response generation.
- TRMNL 800x480 PNG rendering for the current list.
- TRMNL telemetry acceptance for empty bodies or valid JSON.

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

## Runtime Shape

The service is split into these responsibilities:

- Startup loads `AppConfig`, initializes `AppState`, builds the Axum router,
  binds `BIND_ADDR`, and serves requests.
- `AppState` owns shared configuration, a SQLite store handle, and a WhatsApp
  reply client.
- Persistence uses `rusqlite` directly against `DATABASE_PATH` and initializes
  the schema with `CREATE TABLE IF NOT EXISTS`.
- Message interpretation and execution stay independent of WhatsApp payload shapes,
  Meta transport, and HTTP handlers.
- WhatsApp integration targets the official Meta WhatsApp Cloud API only.
- TRMNL integration exposes BYOS display, PNG image, and telemetry endpoints.

## Data Model

SQLite persistence owns one table named `entries` with:

- `id`
- `text`
- `created_at`

Listing order is creation order. Displayed numeric positions are 1-based and map
directly to that creation-ordered list.

The store operations are:

- initialize the `entries` table
- add an entry
- list entries
- remove by exact text, then case-insensitive text if no exact match exists
- remove by displayed numeric position
- clear all entries

Text is stored exactly as supplied to the store; message execution owns trimming
and validation.

## HTTP Surface

The Axum routes are:

- `GET /webhooks/whatsapp`
- `POST /webhooks/whatsapp`
- `GET /api/display`
- `GET /trmnl/list.png`
- `POST /api/log`

TRMNL endpoints that expose display content require the shared `TRMNL_TOKEN`.
WhatsApp verification compares Meta's `hub.verify_token` against
`WHATSAPP_VERIFY_TOKEN` and returns the provided challenge only on a match.

Handler behavior:

- `GET /webhooks/whatsapp` verifies Meta's challenge.
- `POST /webhooks/whatsapp` parses inbound text messages, toggles matching list
  entries, logs reply text, and sends replies.
- `GET /api/display` returns a TRMNL display response containing the list PNG
  URL.
- `GET /trmnl/list.png` renders the current list as an 800x480 PNG.
- `POST /api/log` accepts empty telemetry bodies or valid JSON and rejects
  invalid JSON.

## WhatsApp Components

WhatsApp payload parsing accepts Meta webhook JSON and extracts inbound text
messages with sender, message id, and text body. Non-text messages, status-only
payloads, incomplete text messages, and unsupported top-level shapes do not
produce list changes.

The reply client targets:

- `https://graph.facebook.com/v23.0/{WHATSAPP_PHONE_NUMBER_ID}/messages`

It sends text replies with bearer authentication from `WHATSAPP_ACCESS_TOKEN`.
Debug output intentionally omits the access token.

## Message Behavior

Non-empty inbound text toggles the matching list entry:

- if the trimmed text is absent, add it and reply `"text" added to list.`
- if the trimmed text is present, remove it and reply `"text" removed from list.`

Matching uses exact text first, then case-insensitive text if no exact match
exists.

Supported slash commands are:

- `/list`: return entries in display order
- `/clear`: remove all entries

Empty input is ignored with a no-op reply.

## Verification Expectations

Before a coding bead is considered done, run:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features`
- `cargo nextest run`

If the local environment has a broken `RUSTC_WRAPPER`, clear it for verification
with `RUSTC_WRAPPER=`.
