# TRMNL WhatsApp List

`trmnl-whatsapp-list` is a small Rust 2024 service for one shared SQLite-backed
text list. WhatsApp messages are intended to mutate or query the list, and a
TRMNL device in BYOS mode is intended to display the current list.

The project intentionally stays narrow: one shared list, direct SQLite startup
initialization, official Meta WhatsApp Cloud API integration, and no migration,
fallback, or backward-compatibility layers unless explicitly requested.

## Current Status

Implemented:

- Runtime configuration from environment variables.
- Axum startup that binds `BIND_ADDR` and serves the router.
- SQLite `entries` table initialization and list operations.
- Command parsing and execution for add, list, remove, clear, and help.
- WhatsApp webhook verification for `GET /webhooks/whatsapp`.
- WhatsApp payload parsing for inbound text messages.
- Meta Graph API text reply client.

Not fully wired yet:

- `POST /webhooks/whatsapp` currently returns `501 Not Implemented`.
- `GET /api/display` currently returns `501 Not Implemented`.
- `GET /trmnl/list.png` currently returns `501 Not Implemented`.
- `POST /api/log` currently returns `501 Not Implemented`.

## Prerequisites

- Rust toolchain with Rust 2024 edition support.
- `cargo-nextest` for the required test runner.
- SQLite support through the bundled `rusqlite` dependency.
- A Meta WhatsApp Cloud API app and phone number for end-to-end WhatsApp use.
- A TRMNL device configured for BYOS mode for end-to-end display use.
- A public HTTPS URL for Meta webhook delivery when testing WhatsApp callbacks.

## Configuration

Required environment variables:

- `WHATSAPP_VERIFY_TOKEN`: token compared during Meta webhook verification.
- `WHATSAPP_ACCESS_TOKEN`: Meta Graph API bearer token.
- `WHATSAPP_PHONE_NUMBER_ID`: WhatsApp Business phone number ID.
- `TRMNL_TOKEN`: shared token intended for TRMNL endpoints.
- `PUBLIC_BASE_URL`: public base URL used when returning display asset URLs.

Optional environment variables:

- `DATABASE_PATH`: SQLite database path, default `list.db`.
- `BIND_ADDR`: server bind address, default `127.0.0.1:3000`.

Example local setup with placeholder values:

```sh
export WHATSAPP_VERIFY_TOKEN="choose-a-local-verify-token"
export WHATSAPP_ACCESS_TOKEN="meta-access-token"
export WHATSAPP_PHONE_NUMBER_ID="meta-phone-number-id"
export TRMNL_TOKEN="choose-a-local-trmnl-token"
export PUBLIC_BASE_URL="https://example.test"
export DATABASE_PATH="list.db"
export BIND_ADDR="127.0.0.1:3000"
```

## Run

```sh
cargo run
```

The service loads configuration, initializes the SQLite database, builds the
Axum router, binds `BIND_ADDR`, and serves requests until stopped.

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

## Endpoint Intent

- `GET /webhooks/whatsapp`: verifies Meta's `hub.verify_token` and returns
  `hub.challenge` on a match.
- `POST /webhooks/whatsapp`: intended to parse inbound WhatsApp text messages,
  execute list commands, and reply through the Meta Graph API.
- `GET /api/display`: intended to return a TRMNL BYOS display response.
- `GET /trmnl/list.png`: intended to render the current list as an 800x480 PNG.
- `POST /api/log`: intended to accept TRMNL device telemetry.
