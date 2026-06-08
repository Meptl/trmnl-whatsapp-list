# Design

## Scope

The service owns one shared text list. It does not model users, per-chat lists,
grocery-specific concepts, permissions beyond shared endpoint tokens, or
cross-provider transport abstractions.

Non-empty WhatsApp text is treated as a list entry toggle by default. If the
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

Message interpretation and execution are independent of WhatsApp payloads and
HTTP handlers. Execution depends on the store boundary and returns reply text
that a transport can send.

Persistence owns exact storage and deterministic list mutations. Text trimming
and message interpretation belong outside the store.

WhatsApp payload parsing owns Meta webhook JSON shape only. It extracts inbound
text messages and ignores unsupported non-text or status-only payloads.

The reply client owns Meta Graph API request construction and sending. The app
does not provide a provider fallback or an alternate WhatsApp transport.

HTTP handlers own route-level behavior and should compose configuration, store,
message execution, payload parsing, and transport clients without moving domain
rules into Axum-specific code.

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
backward-compatibility paths unless the user explicitly requests them. Prefer
replacing incomplete approaches with the intended implementation over
supporting both old and new behavior.
