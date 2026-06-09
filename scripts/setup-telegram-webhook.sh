#!/usr/bin/env sh
set -eu

# Registers this deployment's Telegram webhook.
#
# Required environment:
#   TELEGRAM_BOT_TOKEN - token from BotFather
#   WEBHOOK_KEY        - webhook secret sent by Telegram in X-Telegram-Bot-Api-Secret-Token
#   PUBLIC_BASE_URL    - public HTTPS base URL for this service
#
# Optional environment:
#   DROP_PENDING_UPDATES=true - discard queued Telegram updates while registering

if [ -z "${TELEGRAM_BOT_TOKEN:-}" ]; then
  echo "TELEGRAM_BOT_TOKEN is required in the environment" >&2
  exit 2
fi

if [ -z "${WEBHOOK_KEY:-}" ]; then
  echo "WEBHOOK_KEY is required in the environment" >&2
  exit 2
fi

if [ -z "${PUBLIC_BASE_URL:-}" ]; then
  echo "PUBLIC_BASE_URL is required in the environment" >&2
  exit 2
fi

if [ "${#WEBHOOK_KEY}" -gt 256 ] || ! printf '%s' "$WEBHOOK_KEY" | grep -Eq '^[A-Za-z0-9_-]+$'; then
  echo "WEBHOOK_KEY must be 1-256 characters using only A-Z, a-z, 0-9, _, and - for Telegram" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 2
fi

base_url="${PUBLIC_BASE_URL%/}"
case "$base_url" in
  https://*) ;;
  *)
    echo "PUBLIC_BASE_URL must be a public HTTPS URL for Telegram webhooks" >&2
    exit 2
    ;;
esac

webhook_url="$base_url/webhooks/telegram"
api_url="https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/setWebhook"
drop_pending_updates="${DROP_PENDING_UPDATES:-false}"

case "$drop_pending_updates" in
  true|false) ;;
  *)
    echo "DROP_PENDING_UPDATES must be true or false when set" >&2
    exit 2
    ;;
esac

curl --show-error --silent --fail \
  --request POST \
  --header 'Content-Type: application/json' \
  --data-binary @- \
  "$api_url" <<JSON
{
  "url": "$webhook_url",
  "secret_token": "$WEBHOOK_KEY",
  "allowed_updates": ["message"],
  "drop_pending_updates": $drop_pending_updates
}
JSON

echo
printf 'Telegram webhook set to %s\n' "$webhook_url"
