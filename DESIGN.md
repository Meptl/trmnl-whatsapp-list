# Design

## Scope

The service owns one shared text list. It does not model users, per-chat lists,
grocery-specific concepts, permissions beyond shared endpoint tokens, or
cross-provider transport abstractions.

Non-empty active-provider text is treated as a list entry toggle by default. If the
trimmed text is not present, it is added. If it is already present, it is
removed. Matching uses exact text first, then case-insensitive text if no exact
match exists.

Slash commands are reserved for list operations:

- `/list`
- `/clear`

## Persistence

SQLite is initialized directly at startup. The schema is intentionally small:

- table: `entries`
- columns: `id`, `text`, `created_at`

There is no migration framework. If the schema needs to change, that should be a
separate explicitly approved task with a direct plan for existing local data.

The list is displayed in creation order, stabilized by `id` when timestamps tie.
User-facing numeric positions are 1-based indexes into that ordered list.

## Boundaries

Message interpretation and execution are independent of provider payloads and
HTTP handlers. Execution depends on the store boundary and returns reply text
that a transport can send.

Persistence owns exact storage and deterministic list mutations. Text trimming
and message interpretation belong outside the store.

Provider payload parsing owns provider webhook JSON shapes only. WhatsApp parsing
extracts inbound text messages and ignores unsupported non-text or status-only
payloads. Telegram parsing extracts normal `message.text` updates and ignores
edited messages, channel posts, non-text updates, incomplete updates, and
unsupported top-level shapes.

Provider reply clients own request construction and sending. WhatsApp uses the
Meta Graph API. Telegram uses the official Bot API `sendMessage` endpoint. The
app runs exactly one configured provider and does not provide fallback transports.

HTTP handlers own route-level behavior and should compose configuration, store,
message execution, payload parsing, and transport clients without moving domain
rules into Axum-specific code.


## Messaging Providers

The service runs in one active provider mode per deployment. Provider mode is
inferred from environment variables:

- Telegram mode: `WEBHOOK_KEY` and `TELEGRAM_BOT_TOKEN`.
- WhatsApp mode: `WEBHOOK_KEY`, `WHATSAPP_ACCESS_TOKEN`, and
  `WHATSAPP_PHONE_NUMBER_ID`.

Telegram mode is preferred when both provider groups are present. Startup fails
if neither provider group is present or if Telegram is absent and the WhatsApp
group is incomplete. `WHATSAPP_VERIFY_TOKEN` is intentionally not a
compatibility alias; use `WEBHOOK_KEY`.

WhatsApp exposes `GET/POST /webhooks/whatsapp`. Telegram exposes
`POST /webhooks/telegram` and requires `X-Telegram-Bot-Api-Secret-Token` to
match `WEBHOOK_KEY`. Only the active provider route is registered.

## TRMNL BYOS

TRMNL integration targets physical BYOS firmware 1.8.2 with these endpoints:

- setup handshake at `/api/setup`
- display metadata at `/api/display`
- rendered PNG at `/trmnl/list.png`
- telemetry at `/api/log`

The operator chooses `TRMNL_TOKEN` as a server-side environment variable. The
operator does not type that token into the device. During setup, firmware sends
`GET /api/setup` with its `ID` header, and the service returns the token as
`api_key`. Later firmware requests send the same value as the `Access-Token`
header.

The BYOS request flow is:

1. `GET /api/setup` with `ID`.
2. `GET /api/display` with `ID` and `Access-Token`.
3. Fetch the returned `image_url`, currently `/trmnl/list.png`, with `ID` and
   `Access-Token`.
4. `POST /api/log` with `ID` and `Access-Token`.

`PUBLIC_BASE_URL` must be the externally reachable HTTPS base URL the device
uses to fetch returned image URLs. Cloud deployments should bind on an address
appropriate for the host, often `0.0.0.0:$PORT` when the platform injects
`PORT`.

Display content should stay e-ink friendly: a top navigation spacer with the
list title and battery indicator, current entries in creation order, and an
empty state.

TRMNL display, image, and log endpoints require the firmware `Access-Token`
header to match the server-side `TRMNL_TOKEN`.

## Compatibility

Do not add fallback methods, provider compatibility layers, legacy aliases, or
backward-compatibility paths unless the user explicitly requests them. Exactly
one messaging provider is active per deployment, with Telegram preferred when
both provider credential groups exist. Prefer replacing incomplete approaches
with the intended implementation over supporting both old and new behavior.
