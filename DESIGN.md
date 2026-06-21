# Design

## Scope

The service owns one shared text list. It does not model users, per-chat lists,
grocery-specific concepts, permissions beyond shared endpoint tokens, or
cross-provider transport abstractions.

Senders must authorize before list access by sending `/login <CHAT_AUTH_KEY>`.
`/logout` removes only that sender's authorization. Unauthorized messages,
including wrong or malformed login attempts, are silent and do not mutate the
list.

Non-empty authorized configured-provider text is treated as a list entry toggle by
default. If the trimmed text is not present, it is added. If it is already
present, it is removed. Matching uses exact text first, then case-insensitive
text if no exact match exists.

Slash commands are reserved for chat auth and list operations:

- `/login <CHAT_AUTH_KEY>`
- `/logout`
- `/list`
- `/clear`

## Persistence

SQLite is initialized directly at startup. The schema is intentionally small:

- table: `entries`
- columns: `id`, `text`, `created_at`
- table: `authorized_chat_senders`
- columns: `provider`, `sender_id`

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
extracts inbound text messages and uses `from` as the auth sender id. Telegram
parsing extracts normal `message.text` updates, uses `message.from.id` as the
auth sender id, keeps `message.chat.id` as the reply target, and ignores edited
messages, channel posts, non-text updates, incomplete updates, and unsupported
top-level shapes.

Provider reply clients own request construction and sending. WhatsApp uses the
Meta Graph API. Telegram uses the official Bot API `sendMessage` endpoint. The
app runs every configured provider with complete credentials and does not provide
fallback transports.

HTTP handlers own route-level behavior and should compose configuration, store,
chat auth gating, message execution, payload parsing, and transport clients
without moving domain rules into Axum-specific code.


## Messaging Providers

Google Calendar credentials are required for startup:

- `GOOGLE_CALENDAR_CLIENT_ID`
- `GOOGLE_CALENDAR_CLIENT_SECRET`
- `GOOGLE_CALENDAR_REFRESH_TOKEN`

The service registers every provider webhook whose required environment
variables are complete:

- Telegram provider: `WEBHOOK_KEY` and `TELEGRAM_BOT_TOKEN`.
- WhatsApp provider: `WEBHOOK_KEY`, `WHATSAPP_ACCESS_TOKEN`, and
  `WHATSAPP_PHONE_NUMBER_ID`.

When both provider groups are present, both WhatsApp and Telegram endpoints are
active and both update the same shared list. Startup fails if neither provider
group is present or if WhatsApp is the only provider and its group is incomplete.
If Telegram is complete and the WhatsApp group is incomplete, Telegram still
starts by itself. `WHATSAPP_VERIFY_TOKEN` is intentionally not a compatibility
alias; use `WEBHOOK_KEY`.

WhatsApp exposes `GET/POST /webhooks/whatsapp`. Telegram exposes
`POST /webhooks/telegram` and requires `X-Telegram-Bot-Api-Secret-Token` to
match `WEBHOOK_KEY`. Each complete provider group registers its endpoint.

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

Display content should stay e-ink friendly: the battery indicator, the current
list in creation order on the left, and today's Google Calendar events on the
right. The calendar pane is titled `Events - MON DD`, using today's calendar
month and day. All-day events render as `ALL DAY EVENT_TITLE`; timed events render in
chronological 24-hour local form such as `09:30 EVENT_TITLE`. Long titles wrap
with continuation lines indented after the time or all-day prefix.

Google Calendar access uses OAuth refresh-token credentials from environment
variables and the Calendar readonly scope. The service reads the authenticated
user's selected, non-hidden calendars, determines today from the primary
calendar timezone, and fetches live calendar data during display metadata and
image requests. Calendar data is not persisted. Runtime token or Calendar API
failures do not fail TRMNL rendering; the list still renders and the calendar
pane shows `Events unavailable`.

TRMNL display, image, and log endpoints require the firmware `Access-Token`
header to match the server-side `TRMNL_TOKEN`.

## Compatibility

Do not add fallback methods, provider compatibility layers, legacy aliases, or
backward-compatibility paths unless the user explicitly requests them. Each
complete provider credential group can be active in the same deployment. Prefer
replacing incomplete approaches with the intended implementation over supporting
both old and new behavior.
