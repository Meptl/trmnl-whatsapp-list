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
- TRMNL BYOS setup handshake at `GET /api/setup`.
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
- A server-side TRMNL token chosen by the operator.

## Deployment

Run the service on a local machine, VPS, Raspberry Pi, NAS, or similar host that
can keep a Rust service and SQLite database running.

Meta must be able to reach the WhatsApp webhook over public HTTPS at:

- `GET /webhooks/whatsapp` for webhook verification.
- `POST /webhooks/whatsapp` for inbound message delivery.

For a physical TRMNL device in BYOS mode, configure the device with the cloud
base URL. Do not point the device at `/api/display?token=...` or at
`/api/display` directly. Firmware 1.8.2 starts with `GET /api/setup`, then uses
the returned API key when it fetches display metadata, the image, and telemetry.

The cloud deployment must be reachable by the physical device over public HTTPS.
`PUBLIC_BASE_URL` must be that externally reachable HTTPS base URL, for example
`https://trmnl-list.example.com`. The service returns image URLs based on this
value, and the device must be able to fetch those URLs.

## Configuration

Required environment variables:

- `WHATSAPP_VERIFY_TOKEN`: operator-chosen token configured in the Meta webhook
  subscription and compared during verification.
- `WHATSAPP_ACCESS_TOKEN`: Meta Graph API bearer token used to send WhatsApp
  replies.
- `WHATSAPP_PHONE_NUMBER_ID`: WhatsApp Business phone number ID used in the Meta
  send-message URL.
- `TRMNL_TOKEN`: server-side token returned by `GET /api/setup` as `api_key`.
  The operator does not type this token into the device. Firmware sends it back
  on later requests as the `Access-Token` header.
- `PUBLIC_BASE_URL`: externally reachable HTTPS base URL used when returning
  TRMNL image URLs to the physical device.

Optional environment variables:

- `DATABASE_PATH`: SQLite database path, default `list.db`.
- `BIND_ADDR`: server bind address, default `127.0.0.1:3000`. For cloud
  hosting, use an address suitable for the platform, often `0.0.0.0:$PORT` when
  the platform injects `PORT`.

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

Example cloud bind when the host provides `PORT`:

```sh
export BIND_ADDR="0.0.0.0:$PORT"
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

## TRMNL BYOS Device Setup

Use the cloud deployment's base URL in the TRMNL BYOS device configuration, for
example `https://trmnl-list.example.com`. The device should start at that base
URL so firmware can call `GET /api/setup` with its `ID` header.

The BYOS firmware flow is:

1. `GET /api/setup` with `ID`.
2. Read `api_key` from the setup response. This value is the server-side
   `TRMNL_TOKEN`.
3. `GET /api/display` with `ID` and `Access-Token`.
4. Fetch the returned `image_url`, currently `/trmnl/list.png`, with `ID` and
   `Access-Token`.
5. `POST /api/log` with `ID` and `Access-Token`.

A reachable HTTPS server is necessary but not sufficient. Firmware 1.8.2 will
not complete setup if `/api/setup` is missing, and display refreshes will fail
if `/api/display` only accepts a `?token=` query parameter instead of the
firmware `Access-Token` header.

## TRMNL Cloud Smoke Test

Replace `https://HOST` and token placeholders with values from the deployment:

```sh
curl -i https://HOST/api/setup \
  -H "ID: test-device"

curl -i https://HOST/api/display \
  -H "ID: test-device" \
  -H "Access-Token: $TRMNL_TOKEN"

curl -i "<image_url-returned-by-display>" \
  -H "ID: test-device" \
  -H "Access-Token: $TRMNL_TOKEN"

curl -i -X POST https://HOST/api/log \
  -H "ID: test-device" \
  -H "Access-Token: $TRMNL_TOKEN" \
  -H "Content-Type: application/json" \
  --data-binary '{"battery_voltage":"4.12","fw_version":"1.8.2","rssi":"-62","refresh_rate":"60"}'
```

For the image fetch, use the exact `image_url` returned by `/api/display`. It is
currently `https://HOST/trmnl/list.png` when `PUBLIC_BASE_URL=https://HOST`.

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
- `GET /api/setup`: accepts a TRMNL firmware `ID` header and returns setup JSON
  including `api_key`, `friendly_id`, `image_url`, and `filename`.
- `GET /api/display`: requires TRMNL firmware `ID` and `Access-Token` headers
  and returns display JSON whose image URL points at `/trmnl/list.png`.
- `GET /trmnl/list.png`: requires TRMNL firmware `ID` and `Access-Token`
  headers and renders the current list as an 800x480 PNG.
- `POST /api/log`: requires TRMNL firmware `ID` and `Access-Token` headers,
  accepts empty bodies or valid JSON telemetry, and rejects invalid JSON.
