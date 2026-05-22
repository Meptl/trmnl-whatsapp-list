# TRMNL WhatsApp List

`trmnl-whatsapp-list` is a small Rust 2024 service for one shared SQLite-backed
text list. WhatsApp messages toggle list entries, and a TRMNL device in BYOS
mode displays the current list.

The project intentionally stays narrow: one shared list, direct SQLite startup
initialization, official Meta WhatsApp Cloud API integration, and no migration,
fallback, or backward-compatibility layers unless explicitly requested.

## Current Status

Implemented:

- Runtime configuration from environment variables.
- Axum startup that binds `BIND_ADDR` and serves the router.
- SQLite `entries` table initialization and list operations.
- Message text toggling that adds missing entries and removes existing entries.
- Slash commands for `/list` and `/clear`.
- WhatsApp webhook verification for `GET /webhooks/whatsapp`.
- WhatsApp payload parsing for inbound text messages.
- Meta Graph API text reply client.
- WhatsApp list updates and replies for `POST /webhooks/whatsapp`.
- TRMNL BYOS display metadata at `GET /api/display`.
- TRMNL list PNG rendering at `GET /trmnl/list.png`.
- TRMNL telemetry acceptance at `POST /api/log`.

## Prerequisites

- Rust toolchain with Rust 2024 edition support.
- `cargo-nextest` for the required test runner.
- SQLite support through the bundled `rusqlite` dependency.
- A Meta WhatsApp Cloud API app.
- A WhatsApp Business phone number ID for that app.
- A long-lived or permanent Meta access token that can send messages for that
  phone number.
- A webhook verify token chosen by the operator.
- A public HTTPS URL for Meta webhook delivery.
- A TRMNL device configured for BYOS mode.
- A shared TRMNL token chosen by the operator.

## Deployment

Run the service on a local machine, VPS, Raspberry Pi, NAS, or similar host that
can keep a Rust service and SQLite database running.

Meta must be able to reach the WhatsApp webhook over public HTTPS at:

- `GET /webhooks/whatsapp` for webhook verification.
- `POST /webhooks/whatsapp` for inbound message delivery.

TRMNL should point to `/api/display?token=<trmnl-token>`. TRMNL access may be
LAN-only or public depending on the device setup, but `PUBLIC_BASE_URL` must be
the base URL that the device can use to fetch `/trmnl/list.png`.

## Configuration

Required environment variables:

- `WHATSAPP_VERIFY_TOKEN`: operator-chosen token configured in the Meta webhook
  subscription and compared during verification.
- `WHATSAPP_ACCESS_TOKEN`: Meta Graph API bearer token used to send WhatsApp
  replies.
- `WHATSAPP_PHONE_NUMBER_ID`: WhatsApp Business phone number ID used in the Meta
  send-message URL.
- `TRMNL_TOKEN`: shared token required by TRMNL display and image endpoints.
- `PUBLIC_BASE_URL`: base URL used when returning the TRMNL image URL.

Optional environment variables:

- `DATABASE_PATH`: SQLite database path, default `list.db`.
- `BIND_ADDR`: server bind address, default `127.0.0.1:3000`.

Example local setup with placeholder values:

```sh
export WHATSAPP_VERIFY_TOKEN="replace-with-operator-chosen-verify-token"
export WHATSAPP_ACCESS_TOKEN="replace-with-meta-access-token"
export WHATSAPP_PHONE_NUMBER_ID="replace-with-meta-phone-number-id"
export TRMNL_TOKEN="replace-with-operator-chosen-trmnl-token"
export PUBLIC_BASE_URL="https://example.test"
export DATABASE_PATH="list.db"
export BIND_ADDR="127.0.0.1:3000"
```

WhatsApp replies are sent through the Meta Graph API endpoint:

```text
https://graph.facebook.com/v23.0/{WHATSAPP_PHONE_NUMBER_ID}/messages
```

## Run

```sh
cargo run
```

The service loads configuration, initializes the SQLite database, builds the
Axum router, binds `BIND_ADDR`, and serves requests until stopped.

## Local Webhook Exercise

With the server running locally, send a sample inbound WhatsApp text webhook:

```sh
scripts/send-local-whatsapp-webhook.sh
```

The script posts to `http://127.0.0.1:3000/webhooks/whatsapp` by default. Pass a
base URL to target a different local bind address:

```sh
scripts/send-local-whatsapp-webhook.sh http://127.0.0.1:4000
```

The webhook handler sends a WhatsApp reply through Meta as part of normal
processing, so a local run still depends on the configured Meta credentials.

## Check

Required verification commands:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features
cargo nextest run
```

If the local environment has a broken `RUSTC_WRAPPER`, clear it for verification:

```sh
RUSTC_WRAPPER= cargo fmt --check
RUSTC_WRAPPER= cargo clippy --all-targets --all-features
RUSTC_WRAPPER= cargo nextest run
```

## Endpoints

- `GET /webhooks/whatsapp`: verifies Meta's `hub.verify_token` and returns
  `hub.challenge` on a match.
- `POST /webhooks/whatsapp`: parses inbound WhatsApp text messages, toggles the
  matching list entry, and replies through the Meta Graph API.
- `GET /api/display?token=...`: returns a TRMNL BYOS display response whose
  image URL points at `/trmnl/list.png?token=...`.
- `GET /trmnl/list.png?token=...`: renders the current list as an 800x480 PNG.
- `POST /api/log`: accepts empty bodies or valid JSON telemetry and rejects
  invalid JSON.
