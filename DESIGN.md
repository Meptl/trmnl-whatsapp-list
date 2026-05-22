# Design

## Scope

The service owns one shared text list. It does not model users, per-chat lists,
grocery-specific concepts, permissions beyond shared endpoint tokens, or
cross-provider transport abstractions.

Plain unrecognized WhatsApp text is treated as an entry to add. Explicit command
text is reserved for list operations:

- `list`
- `remove <text>`
- `remove <number>`
- `clear`
- `help`

## Persistence

SQLite is initialized directly at startup. The schema is intentionally small:

- table: `entries`
- columns: `id`, `text`, `created_at`

There is no migration framework. If the schema needs to change, that should be a
separate explicitly approved task with a direct plan for existing local data.

The list is displayed in creation order, stabilized by `id` when timestamps tie.
User-facing numeric positions are 1-based indexes into that ordered list.

## Boundaries

Command parsing and execution are independent of WhatsApp payloads and HTTP
handlers. Command execution depends on the store boundary and returns reply text
that a transport can send.

Persistence owns exact storage and deterministic list mutations. Text trimming
and command interpretation belong outside the store.

WhatsApp payload parsing owns Meta webhook JSON shape only. It extracts inbound
text messages and ignores unsupported non-text or status-only payloads.

The reply client owns Meta Graph API request construction and sending. The app
does not provide a provider fallback or an alternate WhatsApp transport.

HTTP handlers own route-level behavior and should compose configuration, store,
command execution, payload parsing, and transport clients without moving domain
rules into Axum-specific code.

## TRMNL BYOS

TRMNL integration is planned around BYOS endpoints:

- display metadata at `/api/display`
- rendered PNG at `/trmnl/list.png`
- telemetry at `/api/log`

Display content should stay e-ink friendly: list title, entry count, current
entries in creation order, an empty state, and a generated timestamp.

TRMNL display and image endpoints are intended to require the shared
`TRMNL_TOKEN`.

## Compatibility

Do not add fallback methods, provider compatibility layers, legacy command
aliases, or backward-compatibility paths unless the user explicitly requests
them. Prefer replacing incomplete approaches with the intended implementation
over supporting both old and new behavior.
