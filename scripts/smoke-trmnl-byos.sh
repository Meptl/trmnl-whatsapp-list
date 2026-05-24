#!/usr/bin/env sh
set -eu

# Usage:
#   TRMNL_TOKEN=replace-with-token scripts/smoke-trmnl-byos.sh https://example.com
#
# Optional:
#   DEVICE_ID=physical-or-test-device-id

if [ "$#" -lt 1 ] || [ -z "${1:-}" ]; then
  echo "usage: TRMNL_TOKEN=... $0 https://example.com" >&2
  exit 2
fi

if [ -z "${TRMNL_TOKEN:-}" ]; then
  echo "TRMNL_TOKEN is required in the environment" >&2
  exit 2
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 2
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 2
fi

base_url="${1%/}"
device_id="${DEVICE_ID:-test-device}"
tmp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT HUP INT TERM

body_file="$tmp_dir/body"
headers_file="$tmp_dir/headers"

request_json() {
  method="$1"
  url="$2"
  data="${3:-}"

  if [ -n "$data" ]; then
    curl --show-error --silent --fail \
      --request "$method" \
      --header "ID: $device_id" \
      --header "Access-Token: $TRMNL_TOKEN" \
      --header "Content-Type: application/json" \
      --data-binary "$data" \
      --output "$body_file" \
      "$url"
  else
    curl --show-error --silent --fail \
      --request "$method" \
      --header "ID: $device_id" \
      --header "Access-Token: $TRMNL_TOKEN" \
      --output "$body_file" \
      "$url"
  fi
}

echo "GET $base_url/api/setup"
curl --show-error --silent --fail \
  --request GET \
  --header "ID: $device_id" \
  --output "$body_file" \
  "$base_url/api/setup"

api_key="$(jq -r '.api_key // empty' "$body_file")"
if [ -z "$api_key" ]; then
  echo "setup response did not include api_key" >&2
  exit 1
fi

if [ "$api_key" != "$TRMNL_TOKEN" ]; then
  echo "setup api_key did not match TRMNL_TOKEN" >&2
  exit 1
fi
echo "OK setup returned matching api_key"

echo "GET $base_url/api/display"
request_json GET "$base_url/api/display"

image_url="$(jq -r '.image_url // empty' "$body_file")"
if [ -z "$image_url" ]; then
  echo "display response did not include image_url" >&2
  exit 1
fi
echo "OK display returned image_url: $image_url"

echo "GET $image_url"
image_status="$(
  curl --show-error --silent \
    --request GET \
    --header "ID: $device_id" \
    --header "Access-Token: $TRMNL_TOKEN" \
    --dump-header "$headers_file" \
    --output "$body_file" \
    --write-out '%{http_code}' \
    "$image_url"
)"

if [ "$image_status" != "200" ]; then
  echo "image request returned HTTP $image_status" >&2
  exit 1
fi

image_content_type="$(
  awk '{ line = tolower($0); if (line ~ /^content-type:/) { sub(/\r$/, "", line); print line; exit } }' "$headers_file"
)"
case "$image_content_type" in
  *image/png*) ;;
  *)
    echo "image response content type was not image/png: $image_content_type" >&2
    exit 1
    ;;
esac
echo "OK image returned HTTP 200 and image/png"

echo "POST $base_url/api/log"
request_json POST "$base_url/api/log" '{"logMessage":"Smoke test display refresh","deviceStatusStamp":"2026-05-24T00:00:00Z","firmwareVersion":"1.8.2","batteryVoltage":4.12,"rssi":-62,"refreshRate":60}'
echo "OK log accepted"
