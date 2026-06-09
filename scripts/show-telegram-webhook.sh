#!/usr/bin/env sh
set -eu

# Shows the current Telegram bot identity and webhook configuration.
#
# Required environment:
#   TELEGRAM_BOT_TOKEN - token from BotFather

if [ -z "${TELEGRAM_BOT_TOKEN:-}" ]; then
  echo "TELEGRAM_BOT_TOKEN is required in the environment" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 2
fi

api_base="https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}"

print_json() {
  if command -v jq >/dev/null 2>&1; then
    jq .
  else
    cat
  fi
}

echo "== Bot identity: getMe =="
curl --show-error --silent --fail "$api_base/getMe" | print_json

echo
echo "== Webhook information: getWebhookInfo =="
curl --show-error --silent --fail "$api_base/getWebhookInfo" | print_json

echo
