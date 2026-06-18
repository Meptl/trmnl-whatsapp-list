# TRMNL WhatsApp List

`trmnl-whatsapp-list` is a TRMNL BYOS service to display a SQLite-backed list.
Configured messaging providers control the list entries: WhatsApp, Telegram, or
both at once.

The project intentionally stays narrow: one shared list, direct SQLite startup
initialization, official Meta WhatsApp Cloud API integration, and no migration,
fallback, or backward-compatibility layers unless explicitly requested.

## Current Status

Implemented:

- Runtime configuration from environment variables.
- Chat-level authorization with `/login` and `/logout`.
- Axum startup that binds `BIND_ADDR` and serves the router.
- SQLite `entries` table initialization and list operations.
- Message text toggling that adds missing entries and removes existing entries.
- Slash commands for `/list` and `/clear`.
- Runtime configuration for WhatsApp, Telegram, or both providers at once.
- WhatsApp webhook verification for `GET /webhooks/whatsapp`.
- WhatsApp payload parsing for inbound text messages.
- Meta Graph API text reply client.
- Telegram webhook secret validation for `POST /webhooks/telegram`.
- Telegram update parsing for inbound text messages.
- Telegram Bot API `sendMessage` reply client.
- Messaging list updates and replies for configured provider webhooks.
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
- A webhook key chosen by the operator. For WhatsApp it is the Meta verify
  token; for Telegram it is the Telegram webhook `secret_token`.
- A public HTTPS URL for configured-provider webhook delivery.
- A TRMNL device configured for BYOS mode.
- A server-side TRMNL token chosen by the operator.

## Deployment

Run the service on a local machine, VPS, Raspberry Pi, NAS, or similar host that
can keep a Rust service and SQLite database running.

The deployment runs every messaging provider with a complete credential group.
If both Telegram and WhatsApp credentials are present, both webhook endpoints are
active and share the same list behavior.

When WhatsApp is configured, Meta must be able to reach:

- `GET /webhooks/whatsapp` for webhook verification.
- `POST /webhooks/whatsapp` for inbound message delivery.

When Telegram is configured, Telegram must be configured to deliver bot updates
to:

- `POST /webhooks/telegram` for inbound message delivery.

For a physical TRMNL device in BYOS mode, configure the device with the same
public URL. Firmware 1.8.2 starts with `GET /api/setup`, then uses
the returned API key when it fetches display metadata, the image, and telemetry.

The cloud deployment must be reachable by the physical device over public HTTPS.
`PUBLIC_BASE_URL` must be that externally reachable HTTPS base URL, for example
`https://trmnl-list.example.com`. The service returns image URLs based on this
value, and the device must be able to fetch those URLs.

## Configuration

Required common environment variables:

- `WEBHOOK_KEY`: operator-chosen webhook secret. For WhatsApp, configure this
  value as Meta's webhook verify token. For Telegram, use it as the Telegram
  webhook `secret_token`; choose only `A-Z`, `a-z`, `0-9`, `_`, and `-` for
  Telegram compatibility.
- `TRMNL_TOKEN`: server-side token returned by `GET /api/setup` as `api_key`.
  The operator does not type this token into the device. Firmware sends it back
  on later requests as the `Access-Token` header.
- `PUBLIC_BASE_URL`: externally reachable HTTPS base URL used when returning
  TRMNL image URLs to the physical device.

Required WhatsApp provider variables:

- `WHATSAPP_ACCESS_TOKEN`: Meta Graph API bearer token used to send WhatsApp
  replies.
- `WHATSAPP_PHONE_NUMBER_ID`: WhatsApp Business phone number ID used in the Meta
  send-message URL.

Required Telegram provider variables:

- `TELEGRAM_BOT_TOKEN`: Bot API token for an operator-created Telegram bot.

Set at least one provider group. When both provider groups are configured, both
webhook endpoints are registered and both providers can update the same list.
Startup fails if no provider group is configured or if WhatsApp is the only
provider with an incomplete credential group. If Telegram is complete and
WhatsApp is incomplete, the service starts with Telegram only.
`WHATSAPP_VERIFY_TOKEN` is not supported; use `WEBHOOK_KEY`.

Optional environment variables:

- `CHAT_AUTH_KEY`: preshared chat login key. Senders must send
  `/login <CHAT_AUTH_KEY>` before any list command or item text is accepted. If
  omitted, no sender can log in until it is set and the service restarts.
- `DATABASE_PATH`: SQLite database path, default `list.db`.
- `BIND_ADDR`: server bind address, default `127.0.0.1:3000`. For cloud
  hosting, use an address suitable for the platform, often `0.0.0.0:$PORT` when
  the platform injects `PORT`.

Example local setup with placeholder values:

```sh
export WEBHOOK_KEY="replace-with-operator-chosen-webhook-key"
export CHAT_AUTH_KEY="replace-with-preshared-chat-login-key"
export WHATSAPP_ACCESS_TOKEN="replace-with-meta-access-token"
export WHATSAPP_PHONE_NUMBER_ID="replace-with-meta-phone-number-id"
export TRMNL_TOKEN="replace-with-operator-chosen-trmnl-token"
export PUBLIC_BASE_URL="https://example.test"
export DATABASE_PATH="list.db"
export BIND_ADDR="127.0.0.1:3000"
```

Example Telegram setup with placeholder values:

```sh
export WEBHOOK_KEY="replace-with-telegram-compatible-webhook-key"
export CHAT_AUTH_KEY="replace-with-preshared-chat-login-key"
export TELEGRAM_BOT_TOKEN="replace-with-botfather-token"
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

## Telegram Setup

Create a bot with BotFather, copy its bot token into `TELEGRAM_BOT_TOKEN`, and
set this service as the bot webhook. `PUBLIC_BASE_URL` must be a public HTTPS
base URL reachable by Telegram, not `localhost`.

With the required Telegram environment variables exported, run:

```sh
scripts/setup-telegram-webhook.sh
```

The script registers:

- webhook URL: `$PUBLIC_BASE_URL/webhooks/telegram`
- webhook secret: `$WEBHOOK_KEY`, sent by Telegram as
  `X-Telegram-Bot-Api-Secret-Token`
- allowed updates: `message`

Set `DROP_PENDING_UPDATES=true` if you want Telegram to discard queued updates
while registering the webhook:

```sh
DROP_PENDING_UPDATES=true scripts/setup-telegram-webhook.sh
```

To inspect the bot identity and current webhook status returned by Telegram:

```sh
scripts/show-telegram-webhook.sh
```

Equivalent manual command:

```sh
curl -X POST "https://api.telegram.org/bot$TELEGRAM_BOT_TOKEN/setWebhook" \
  -H "Content-Type: application/json" \
  --data-binary "{\"url\":\"$PUBLIC_BASE_URL/webhooks/telegram\",\"secret_token\":\"$WEBHOOK_KEY\",\"allowed_updates\":[\"message\"]}"
```

After registration, message the bot in Telegram and send `/login <CHAT_AUTH_KEY>`.
Telegram should POST the update to `/webhooks/telegram`; the service validates
the secret header, authorizes the sender, then accepts later list mutations and
replies with `sendMessage`.

For group chats, disable the bot's BotFather privacy mode if you want plain item
messages such as `Moo` to update the list. With privacy mode enabled, Telegram
usually delivers commands like `/login`, replies to the bot, and mentions, but
not regular group messages; the service cannot process messages Telegram does
not send to the webhook. In BotFather, use `/setprivacy`, choose the bot, and
select `Disable`.

Telegram handling accepts normal `message.text` updates from any chat type delivered
to the bot. Edited messages, channel posts, non-text updates, and incomplete
updates are ignored.

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
processing, so a local WhatsApp-enabled run still depends on the configured Meta
credentials. When Telegram is configured, post valid Telegram update JSON to
`/webhooks/telegram` with `X-Telegram-Bot-Api-Secret-Token: $WEBHOOK_KEY`:

```sh
WEBHOOK_KEY="replace-with-telegram-webhook-secret" \
  scripts/send-local-telegram-webhook.sh
```

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

The same BYOS flow can be checked with the smoke script:

```sh
TRMNL_TOKEN="replace-with-operator-chosen-trmnl-token" \
  scripts/smoke-trmnl-byos.sh https://HOST
```

Set `DEVICE_ID` to override the default `test-device` ID header.

## Local TRMNL Image Preview

With the server running locally, fetch the rendered PNG and open it with `mpv`:

```sh
TRMNL_TOKEN="replace-with-operator-chosen-trmnl-token" \
  scripts/preview-trmnl-image.sh
```

The script defaults to `http://127.0.0.1:3000`, fetches `/api/display` with a
sample `Battery-Voltage: 4.12` header, downloads the returned `image_url`, and
saves `trmnl-list.png`. Override these as needed:

```sh
TRMNL_TOKEN="replace-with-operator-chosen-trmnl-token" \
DEVICE_ID="test-device" \
BATTERY_VOLTAGE="3.85" \
OUTPUT="preview.png" \
  scripts/preview-trmnl-image.sh http://127.0.0.1:3000
```

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

- When WhatsApp is configured, `GET /webhooks/whatsapp` verifies Meta's `hub.verify_token`
  against `WEBHOOK_KEY` and returns `hub.challenge` on a match.
- When WhatsApp is configured, `POST /webhooks/whatsapp` parses inbound WhatsApp text messages,
  requires sender login before list access, toggles matching list entries, and
  replies through the Meta Graph API.
- When Telegram is configured, `POST /webhooks/telegram` requires
  `X-Telegram-Bot-Api-Secret-Token: WEBHOOK_KEY`, parses inbound `message.text`
  updates, requires sender login before list access, toggles matching list
  entries, and replies through `sendMessage`.
- `GET /api/setup`: accepts a TRMNL firmware `ID` header and returns setup JSON
  including `api_key`, `friendly_id`, `image_url`, and `filename`.
- `GET /api/display`: requires TRMNL firmware `ID` and `Access-Token` headers
  and returns display JSON whose image URL points at `/trmnl/list.png`.
- `GET /trmnl/list.png`: requires TRMNL firmware `ID` and `Access-Token`
  headers and renders the current list as an 800x480 PNG.
- `POST /api/log`: requires TRMNL firmware `ID` and `Access-Token` headers,
  accepts empty bodies or valid JSON telemetry, and rejects invalid JSON.
