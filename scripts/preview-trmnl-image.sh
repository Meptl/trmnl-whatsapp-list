#!/usr/bin/env sh
set -eu

# Usage:
#   TRMNL_TOKEN=replace-with-token scripts/preview-trmnl-image.sh [http://127.0.0.1:3000]
#
# Optional:
#   DEVICE_ID=physical-or-test-device-id
#   BATTERY_VOLTAGE=4.12
#   OUTPUT=trmnl-list.png

if [ -z "${TRMNL_TOKEN:-}" ]; then
  echo "TRMNL_TOKEN is required in the environment" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 2
fi

if ! command -v mpv >/dev/null 2>&1; then
  echo "mpv is required" >&2
  exit 2
fi

base_url="${1:-http://127.0.0.1:3000}"
base_url="${base_url%/}"
device_id="${DEVICE_ID:-test-device}"
battery_voltage="${BATTERY_VOLTAGE:-4.12}"
output="${OUTPUT:-trmnl-list.png}"

curl --show-error --silent --fail \
  --request GET \
  --header "ID: $device_id" \
  --header "Access-Token: $TRMNL_TOKEN" \
  --header "battery-voltage: $battery_voltage" \
  --output "$output" \
  "$base_url/trmnl/list.png"

echo "Saved preview to $output"
mpv --keep-open=yes "$output"
