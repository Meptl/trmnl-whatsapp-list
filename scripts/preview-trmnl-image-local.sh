#!/usr/bin/env sh
set -eu

# Usage:
#   TRMNL_TOKEN=replace-with-token scripts/preview-trmnl-image-local.sh
#
# Optional:
#   LOCAL_BASE_URL=http://127.0.0.1:3000
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

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 2
fi

if ! command -v mpv >/dev/null 2>&1; then
  echo "mpv is required" >&2
  exit 2
fi

base_url="${LOCAL_BASE_URL:-http://127.0.0.1:3000}"
base_url="${base_url%/}"
device_id="${DEVICE_ID:-test-device}"
battery_voltage="${BATTERY_VOLTAGE:-4.12}"
output="${OUTPUT:-trmnl-list.png}"
tmp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT HUP INT TERM

display_json="$tmp_dir/display.json"

curl --show-error --silent --fail \
  --request GET \
  --header "ID: $device_id" \
  --header "Access-Token: $TRMNL_TOKEN" \
  --header "Battery-Voltage: $battery_voltage" \
  --output "$display_json" \
  "$base_url/api/display"

image_url="$(jq -r '.image_url // empty' "$display_json")"
if [ -z "$image_url" ]; then
  echo "display response did not include image_url" >&2
  exit 1
fi

image_path_and_query="$(printf '%s' "$image_url" | sed -E 's#^[A-Za-z][A-Za-z0-9+.-]*://[^/]*##')"
case "$image_path_and_query" in
  /*) ;;
  *)
    echo "display image_url did not include a fetchable path: $image_url" >&2
    exit 1
    ;;
esac
fetch_image_url="$base_url$image_path_and_query"

echo "Display metadata URL: $base_url/api/display"
echo "Returned image_url: $image_url"
echo "Fetching preview image from local URL: $fetch_image_url"
curl --show-error --silent --fail \
  --request GET \
  --header "ID: $device_id" \
  --header "Access-Token: $TRMNL_TOKEN" \
  --output "$output" \
  "$fetch_image_url"

echo "Saved preview to $output"
mpv --keep-open=yes "$output"
