# TRMNL WhatsApp List Plan

## Summary

Build a single Rust 2024 service that maintains a simple SQLite-backed list of
string entries. Users send WhatsApp messages to add, view, and remove entries.
A TRMNL device in BYOS mode displays the current list.

The app is intentionally small: no migrations, no multi-list model, no
grocery-specific language, and no backward compatibility layer.

## What You Need To Bring

- A Meta WhatsApp Cloud API setup:
  - WhatsApp Business app
  - Business phone number ID
  - Permanent or long-lived access token
  - Webhook verify token chosen by you
  - Public HTTPS URL for Meta to call your webhook
- A TRMNL device configured for BYOS mode:
  - Device points to this app's `/api/display` endpoint
  - Shared TRMNL token chosen by you
- A place to run the service:
  - Local machine, VPS, Raspberry Pi, NAS, or similar
  - Public access is required for WhatsApp webhooks
  - TRMNL access can be LAN-only or public, depending on device setup

## Configuration

Use environment variables:

- `WHATSAPP_VERIFY_TOKEN`: token Meta sends during webhook verification
- `WHATSAPP_ACCESS_TOKEN`: Meta Graph API bearer token
- `WHATSAPP_PHONE_NUMBER_ID`: WhatsApp Business phone number ID
- `TRMNL_TOKEN`: shared token required by TRMNL endpoints
- `PUBLIC_BASE_URL`: public base URL used to return TRMNL image URLs
- `DATABASE_PATH`: SQLite file path, default `list.db`
- `BIND_ADDR`: server bind address, default `127.0.0.1:3000`

## Implementation Changes

- Use Axum for HTTP routing.
- Use `rusqlite` for SQLite persistence with direct startup initialization:
  - Create one table if missing: `entries`
  - Columns: `id`, `text`, `created_at`
  - No migration framework
- Implement list operations:
  - add entry
  - list entries
  - remove entry by exact or case-insensitive text match
  - remove entry by numeric list position
  - clear all entries
- Implement WhatsApp bot commands:
  - Plain text adds an entry
  - `list` returns all entries
  - `remove milk` removes matching text
  - `remove 2` removes the second displayed entry
  - `clear` removes all entries
  - `help` returns supported commands
- Implement Meta WhatsApp endpoints:
  - `GET /webhooks/whatsapp` verifies `hub.verify_token` and returns `hub.challenge`
  - `POST /webhooks/whatsapp` receives text messages, applies commands, and replies via Meta Graph API
- Implement TRMNL BYOS endpoints:
  - `GET /api/display?token=...` returns a `trmnl::DisplayResponse`
  - `GET /trmnl/list.png?token=...` renders the current entries as an 800x480 image
  - `POST /api/log` accepts device telemetry
- Render a simple e-ink friendly TRMNL screen:
  - title: `List`
  - entry count
  - current entries in creation order
  - empty state: `No entries`
  - generated timestamp

## Test Plan

- Unit tests for command parsing:
  - plain text add
  - `list`
  - `remove <text>`
  - `remove <number>`
  - `clear`
  - `help`
- Persistence tests with temporary SQLite files:
  - database initializes from empty file
  - add/list/remove/clear work
  - matching is deterministic when duplicate text exists
- HTTP handler tests:
  - WhatsApp verification succeeds with correct token
  - WhatsApp verification fails with wrong token
  - inbound WhatsApp text applies the expected operation
  - TRMNL display rejects invalid token
  - TRMNL display returns a valid response
- Required final checks:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features`
  - `cargo nextest run`

## Assumptions

- TRMNL target is BYOS mode using the Rust `trmnl` crate.
- WhatsApp provider is the official Meta WhatsApp Cloud API.
- The app manages one shared list, not per-user lists.
- Plain unrecognized text should add a list entry.
- SQLite schema can be recreated manually if it ever needs to change.
