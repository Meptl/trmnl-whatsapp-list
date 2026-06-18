# Architecture

## Purpose

`trmnl-whatsapp-list` is a small Rust 2024 service for one shared list of text
entries. It is designed so messages from any configured provider, WhatsApp and/or
Telegram, toggle list entries, and a TRMNL device in BYOS mode displays the
current list.

The service intentionally avoids migrations, multi-list modeling, provider
fallbacks, and backward compatibility layers.

## Current State

The implemented foundation currently includes:

- Crate-level `#![forbid(unsafe_code)]`.
- Runtime configuration loaded from environment variables, including
  WhatsApp and/or Telegram provider setup and optional `CHAT_AUTH_KEY` chat
  login configuration.
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
- Telegram Bot API webhook parsing for normal `message.text` updates.
- Telegram Bot API `sendMessage` reply request construction and sending through
  the Telegram webhook POST flow.
- TRMNL BYOS setup response generation.
- TRMNL BYOS display response generation.
- TRMNL 800x480 PNG rendering for the current list.
- TRMNL telemetry acceptance for empty bodies or valid JSON.

## Configuration

Configuration is owned by `src/config.rs`.

Required common environment variables:

- `WEBHOOK_KEY`
- `TRMNL_TOKEN`
- `PUBLIC_BASE_URL`

Required WhatsApp provider variables:

- `WHATSAPP_ACCESS_TOKEN`
- `WHATSAPP_PHONE_NUMBER_ID`

Required Telegram provider variables:

- `TELEGRAM_BOT_TOKEN`

Optional environment variables:

- `CHAT_AUTH_KEY`, preshared chat login key. If omitted, chat auth remains
  enforced and no sender can log in.
- `DATABASE_PATH`, defaulting to `list.db`
- `BIND_ADDR`, defaulting to `127.0.0.1:3000`

`TRMNL_TOKEN` is a server-side secret. Operators do not type it into the
physical TRMNL device. `GET /api/setup` returns it as `api_key`, and firmware
sends it back on later requests as the `Access-Token` header.

`PUBLIC_BASE_URL` must be the externally reachable HTTPS base URL for the cloud
deployment because display responses use it to build the device-fetchable image
URL. `BIND_ADDR` should match the hosting platform; cloud hosts often require
`0.0.0.0:$PORT` when they inject `PORT`.

Missing common required variables return `ConfigError::MissingRequiredVariable`
with the variable name. Provider inference enables every complete provider group
and returns typed errors for missing provider groups or for an incomplete
WhatsApp-only provider group. Invalid Unicode in environment keys or values
returns `ConfigError::InvalidUnicode`. Secret values are stored in
`SecretString`, whose `Debug` implementation prints only `[redacted]`.

Tests use `AppConfig::from_pairs` so configuration behavior can be verified
without mutating process-global environment variables.

## Runtime Shape

The service is split into these responsibilities:

- Startup loads `AppConfig`, initializes `AppState`, builds the Axum router,
  binds `BIND_ADDR`, and serves requests.
- `AppState` owns shared configuration, a SQLite store handle, and configured
  provider reply clients.
- Persistence uses `rusqlite` directly against `DATABASE_PATH` and initializes
  the schema with `CREATE TABLE IF NOT EXISTS`.
- Message interpretation, chat auth gating, and execution stay independent of
  provider payload shapes, provider transports, and HTTP handlers.
- WhatsApp integration targets the official Meta WhatsApp Cloud API only.
- Telegram integration targets the official Telegram Bot API only.
- Every complete provider webhook route is registered; when both provider
  credential groups are configured, both are active.
- TRMNL integration exposes BYOS display, PNG image, and telemetry endpoints.

## Data Model

SQLite persistence owns one list table named `entries` with:

- `id`
- `text`
- `created_at`

SQLite persistence also owns one chat auth table named
`authorized_chat_senders` with:

- `provider`
- `sender_id`

Listing order is creation order. Displayed numeric positions are 1-based and map
directly to that creation-ordered list.

The store operations are:

- initialize the `entries` table
- add an entry
- list entries
- remove by exact text, then case-insensitive text if no exact match exists
- remove by displayed numeric position
- clear all entries
- check, add, and remove authorized chat senders by provider and sender id

Text is stored exactly as supplied to the store; message execution owns trimming
and validation.

## HTTP Surface

The common Axum routes are:
- `GET /api/setup`
- `GET /api/display`
- `GET /trmnl/list.png`
- `POST /api/log`

Provider routes depend on which provider credential groups are complete:

- WhatsApp configured: `GET /webhooks/whatsapp` and `POST /webhooks/whatsapp`
- Telegram configured: `POST /webhooks/telegram`

TRMNL display, image, and log endpoints require firmware headers. `ID` is
required for all TRMNL BYOS endpoints. `Access-Token` is required for display,
image, and log requests and must match the server-side `TRMNL_TOKEN`.
WhatsApp verification compares Meta's `hub.verify_token` against `WEBHOOK_KEY`
and returns the provided challenge only on a match. Telegram webhook handling
requires `X-Telegram-Bot-Api-Secret-Token` to match `WEBHOOK_KEY`.

Handler behavior:

- `GET /webhooks/whatsapp` verifies Meta's challenge.
- `POST /webhooks/whatsapp` parses inbound text messages, authorizes senders via
  `/login <CHAT_AUTH_KEY>`, gates list access by sender auth state, logs reply
  text, and sends replies.
- `POST /webhooks/telegram` parses normal `message.text` updates, authorizes
  senders via `/login <CHAT_AUTH_KEY>`, gates list access by sender auth state,
  logs reply text, and sends replies.
- `GET /api/setup` returns TRMNL setup JSON with `api_key`, `friendly_id`,
  `image_url`, and `filename`.
- `GET /api/display` returns TRMNL display JSON containing the list PNG URL.
- `GET /trmnl/list.png` renders the current list as an 800x480 PNG.
- `POST /api/log` accepts empty telemetry bodies or valid JSON and rejects
  invalid JSON.

The physical BYOS firmware flow is:

1. `GET /api/setup` with `ID`.
2. `GET /api/display` with `ID` and `Access-Token`.
3. Fetch the returned `image_url`, currently `/trmnl/list.png`, with `ID` and
   `Access-Token`.
4. `POST /api/log` with `ID` and `Access-Token`.

A publicly reachable deployment is not enough on its own. Firmware 1.8.2 needs
the setup endpoint, and subsequent display requests use header authentication.
An implementation that only accepts `/api/display?token=...` does not satisfy
the physical-device flow.

## Messaging Components

WhatsApp payload parsing accepts Meta webhook JSON and extracts inbound text
messages with sender, message id, and text body. Non-text messages, status-only
payloads, incomplete text messages, and unsupported top-level shapes do not
produce list changes.

Telegram payload parsing accepts Bot API Update JSON and extracts normal
`message.text` updates with chat id, sender user id, message id, and text. The
sender user id is the auth identity; the chat id remains the reply target.
Edited messages, channel posts, non-text messages, incomplete messages, and
unsupported top-level shapes do not produce list changes.

The reply client targets:

- `https://graph.facebook.com/v23.0/{WHATSAPP_PHONE_NUMBER_ID}/messages`

It sends text replies with bearer authentication from `WHATSAPP_ACCESS_TOKEN`.
Debug output intentionally omits the access token.

The Telegram reply client targets:

```text
https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendMessage
```

It sends JSON text replies with `chat_id` and `text`. Debug output intentionally
omits the bot token.

## Message Behavior

Inbound text from any configured provider is ignored until that provider sender id is
authorized. `/login <CHAT_AUTH_KEY>` authorizes a sender, `/logout` removes that
sender's authorization, and wrong or malformed login attempts are silent. If
`CHAT_AUTH_KEY` is omitted, no sender can log in until the variable is set and
the service restarts.

Non-empty authorized inbound text from any configured provider toggles the matching list
entry:

- if the trimmed text is absent, add it and reply `"text" added to list.`
- if the trimmed text is present, remove it and reply `"text" removed from list.`

Matching uses exact text first, then case-insensitive text if no exact match
exists.

Supported slash commands are:

- `/login <CHAT_AUTH_KEY>`: authorize the provider sender id
- `/logout`: remove authorization for the provider sender id
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
