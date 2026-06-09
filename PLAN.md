# Plan

## Context

The application currently supports one shared SQLite-backed list controlled through WhatsApp webhooks and displayed by TRMNL BYOS endpoints. The user wants Telegram support, but chose an exactly-one-provider runtime model: each deployment runs either WhatsApp or Telegram messaging, not both at the same time. Telegram will use the official Telegram Bot API: an operator-created bot receives webhook callbacks and replies through `sendMessage`; there is no user login session.

## Findings / Reuse

- `src/commands.rs` already contains provider-neutral message behavior:
  - trim inbound text
  - `/list`
  - `/clear`
  - toggle text entries
  - produce reply text
- `src/store.rs` already owns the single shared SQLite `entries` table and list mutations. No schema change is needed.
- `src/whatsapp.rs` currently owns WhatsApp-specific webhook payload parsing and Meta Graph API reply request construction.
- `src/http.rs` currently owns Axum route registration, WhatsApp verification/webhook handling, and the common webhook processing loop.
- `src/config.rs` currently loads WhatsApp, TRMNL, public base URL, database path, and bind address from env vars. It is the right place to model exactly-one messaging provider configuration.
- `src/app.rs` currently stores `AppConfig`, `StoreHandle`, and a WhatsApp reply client. It will need to store the active provider client/config shape.
- `README.md`, `DESIGN.md`, and `ARCHITECTURE.md` currently describe WhatsApp-only messaging and must be updated to describe exactly-one WhatsApp-or-Telegram provider behavior.
- `Cargo.toml` already has `reqwest`, `serde`, `serde_json`, `axum`, and `tokio`; no new dependency is expected.

## Decisions / Assumptions

- Runtime provider mode: exactly one messaging provider per deployment.
- Provider selection: infer from env groups, not a `MESSAGE_PROVIDER` env var.
- Shared list: WhatsApp and Telegram modes use the same SQLite-backed shared list model.
- Webhook secret env var: replace `WHATSAPP_VERIFY_TOKEN` with provider-neutral `WEBHOOK_KEY`.
- Backward compatibility: do not add a `WHATSAPP_VERIFY_TOKEN` alias.
- WhatsApp env group:
  - `WEBHOOK_KEY`
  - `WHATSAPP_ACCESS_TOKEN`
  - `WHATSAPP_PHONE_NUMBER_ID`
- Telegram env group:
  - `WEBHOOK_KEY`
  - `TELEGRAM_BOT_TOKEN`
- Startup fails if neither provider group is present, both provider groups are present, or the active provider group is incomplete.
- Active routes: only register the active provider's webhook route.
- Telegram webhook route: `POST /webhooks/telegram`.
- Telegram webhook authentication: require `X-Telegram-Bot-Api-Secret-Token` to match `WEBHOOK_KEY`.
- Telegram update handling: handle only normal `message.text` updates; ignore edited messages, channel posts, non-text updates, and unsupported shapes.
- Telegram chat scope: accept normal `message.text` from all chat types that Telegram delivers to the bot.
- Telegram group commands: support only exact `/list` and `/clear`; do not support `/list@BotUsername` or `/clear@BotUsername` initially.
- Telegram replies: send normal `sendMessage` requests with `{ chat_id, text }`; do not reply-thread to the triggering message initially.
- Telegram setup docs: document the operator-run `setWebhook` curl command; do not add a helper script.

## Risks / Edge Cases

- Renaming `WHATSAPP_VERIFY_TOKEN` to `WEBHOOK_KEY` is a breaking configuration change by design.
- Exactly-one provider mode means this plan does not allow WhatsApp and Telegram simultaneously in the same running process.
- Telegram group messages can mutate the list when the bot receives normal text messages; this is explicitly accepted by the chosen all-message-chat scope.
- Telegram clients often display group bot commands as `/command@BotUsername`; this plan intentionally ignores those until a bot username configuration is requested.
- Long `/list` replies may exceed Telegram's 4096-character `sendMessage` text limit. Existing unsplit reply behavior should remain unless implementation/testing forces a narrower handling decision.
- Telegram `setWebhook` uses a `secret_token` whose allowed characters are limited to `A-Z`, `a-z`, `0-9`, `_`, and `-`; docs should mention choosing a `WEBHOOK_KEY` compatible with Telegram when using Telegram mode.

## Plan:

1. Update configuration modeling in `src/config.rs`.
   - Replace WhatsApp-specific verify token loading with shared `WEBHOOK_KEY`.
   - Add a provider enum, for example `MessagingProviderConfig`, with WhatsApp and Telegram variants.
   - Infer the active provider from provider-specific env vars.
   - Validate exactly-one provider group.
   - Keep TRMNL, `PUBLIC_BASE_URL`, `DATABASE_PATH`, and `BIND_ADDR` behavior unchanged.
   - Update config tests for WhatsApp mode, Telegram mode, missing provider, both providers, incomplete groups, secret redaction, and the removed `WHATSAPP_VERIFY_TOKEN` behavior.

2. Introduce light shared messaging code.
   - Add a small provider-neutral inbound text message type carrying reply target, message id, and text.
   - Add or move a shared async processing helper that accepts parsed inbound text messages and a provider-specific reply closure.
   - Reuse `parse_command()` and `execute_command()` from `src/commands.rs`.
   - Preserve existing behavior where reply-send failures are logged but do not fail the webhook after the list mutation succeeds.

3. Adapt WhatsApp code to the shared messaging shape.
   - Keep WhatsApp payload parsing and Meta reply client provider-specific in `src/whatsapp.rs`.
   - Return provider-neutral inbound text messages from WhatsApp parsing, or convert immediately before calling the shared processing helper.
   - Use `WEBHOOK_KEY` for WhatsApp GET webhook verification.
   - Keep Meta Graph API request shape and reply behavior unchanged.
   - Update WhatsApp parser/client tests to reflect the shared verify key rename and any type moves.

4. Add Telegram provider support in a new `src/telegram.rs` module.
   - Parse Telegram webhook JSON as an Update object.
   - Extract only `message.text` updates with `message.chat.id` and `message.message_id`.
   - Ignore edited messages, channel posts, non-text updates, incomplete messages, and unsupported top-level shapes without panicking.
   - Return typed invalid-JSON errors for malformed JSON.
   - Add a `TelegramReplyClient` that builds and sends `POST https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendMessage` with JSON `{ "chat_id": ..., "text": ... }`.
   - Redact the bot token in debug output and errors where applicable.
   - Add unit tests for parsing, ignored updates, invalid JSON, request construction, and debug redaction.

5. Update application state in `src/app.rs`.
   - Store the active messaging provider client/config instead of an unconditional WhatsApp client.
   - Initialize only the active provider client.
   - Keep store initialization and TRMNL state behavior unchanged.
   - Update app-state tests for both provider modes.

6. Update routing and HTTP handlers in `src/http.rs`.
   - Register only the active provider webhook route:
     - WhatsApp mode: `GET/POST /webhooks/whatsapp`
     - Telegram mode: `POST /webhooks/telegram`
   - Add Telegram webhook handler that validates `X-Telegram-Bot-Api-Secret-Token` against `WEBHOOK_KEY` before processing.
   - Return `403` for missing/wrong Telegram secret header.
   - Return `400` for invalid Telegram JSON.
   - Return `200` for valid but ignored/no-message Telegram updates.
   - Return `500` for command/store failures.
   - Reuse the shared message processing helper for both providers.
   - Update HTTP tests for route registration, inactive route absence, Telegram secret validation, Telegram processing, ignored updates, invalid JSON, and reply failure acknowledgement.

7. Update documentation.
   - In `README.md`, replace WhatsApp-only configuration with exactly-one WhatsApp-or-Telegram provider configuration.
   - Document `WEBHOOK_KEY` replacing `WHATSAPP_VERIFY_TOKEN`.
   - Document Telegram BotFather bot creation at a high level.
   - Document Telegram `setWebhook` curl using `PUBLIC_BASE_URL/webhooks/telegram`, `WEBHOOK_KEY` as `secret_token`, and `allowed_updates=["message"]`.
   - Update endpoint lists to show provider-dependent webhook routes.
   - Update local exercise notes as needed; do not add a helper script.
   - Update `DESIGN.md` and `ARCHITECTURE.md` to reflect exactly-one provider support and the light shared messaging pipeline.

8. Verify.
   - Run `cargo fmt --check`.
   - Run `cargo clippy --all-targets --all-features`.
   - Run `cargo nextest run`.
   - If `RUSTC_WRAPPER` is broken locally, rerun with `RUSTC_WRAPPER=` as documented.
