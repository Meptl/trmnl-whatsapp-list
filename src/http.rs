use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use image::ImageEncoder;
use serde::Deserialize;

use crate::app::AppState;
use crate::commands::{CommandExecutionError, execute_command, parse_command};
use crate::store::StoreHandle;
use crate::whatsapp::{WhatsAppPayloadError, WhatsAppReplyError, parse_inbound_text_messages};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/webhooks/whatsapp",
            get(whatsapp_verify).post(whatsapp_webhook),
        )
        .route("/api/display", get(trmnl_display))
        .route("/trmnl/list.png", get(trmnl_image))
        .route("/api/log", post(trmnl_log))
        .with_state(state)
}

#[derive(Deserialize)]
struct WhatsAppVerifyQuery {
    #[serde(rename = "hub.verify_token")]
    verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    challenge: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrmnlFirmwareHeaders {
    device_id: String,
    access_token: Option<String>,
    battery_voltage: Option<String>,
    fw_version: Option<String>,
    rssi: Option<String>,
    refresh_rate: Option<String>,
    model: Option<String>,
    width: Option<String>,
    height: Option<String>,
    update_source: Option<String>,
    temperature_profile: Option<String>,
    sensors: Option<String>,
}

impl TrmnlFirmwareHeaders {
    fn from_headers(headers: &HeaderMap) -> Result<Self, StatusCode> {
        Ok(Self {
            device_id: required_header(headers, "id", StatusCode::BAD_REQUEST)?,
            access_token: optional_header(headers, "access-token"),
            battery_voltage: optional_header(headers, "battery-voltage"),
            fw_version: optional_header(headers, "fw-version"),
            rssi: optional_header(headers, "rssi"),
            refresh_rate: optional_header(headers, "refresh-rate"),
            model: optional_header(headers, "model"),
            width: optional_header(headers, "width"),
            height: optional_header(headers, "height"),
            update_source: optional_header(headers, "update-source"),
            temperature_profile: optional_header(headers, "temperature-profile"),
            sensors: optional_header(headers, "sensors"),
        })
    }

    fn require_access_token(&self) -> Result<&str, StatusCode> {
        self.access_token.as_deref().ok_or(StatusCode::FORBIDDEN)
    }

    fn validate_access_token(&self, expected_token: &str) -> Result<(), StatusCode> {
        match self.require_access_token() {
            Ok(access_token) if access_token == expected_token => Ok(()),
            _ => Err(StatusCode::FORBIDDEN),
        }
    }

    fn telemetry_summary(&self) -> Option<String> {
        let mut telemetry = Vec::new();
        push_optional_telemetry(&mut telemetry, "battery-voltage", &self.battery_voltage);
        push_optional_telemetry(&mut telemetry, "fw-version", &self.fw_version);
        push_optional_telemetry(&mut telemetry, "rssi", &self.rssi);
        push_optional_telemetry(&mut telemetry, "refresh-rate", &self.refresh_rate);
        push_optional_telemetry(&mut telemetry, "model", &self.model);
        push_optional_telemetry(&mut telemetry, "width", &self.width);
        push_optional_telemetry(&mut telemetry, "height", &self.height);
        push_optional_telemetry(&mut telemetry, "update-source", &self.update_source);
        push_optional_telemetry(
            &mut telemetry,
            "temperature-profile",
            &self.temperature_profile,
        );
        push_optional_telemetry(&mut telemetry, "sensors", &self.sensors);

        if telemetry.is_empty() {
            None
        } else {
            Some(telemetry.join(", "))
        }
    }
}

fn push_optional_telemetry(telemetry: &mut Vec<String>, name: &str, value: &Option<String>) {
    if let Some(value) = value {
        telemetry.push(format!("{name}={value}"));
    }
}

fn required_header(
    headers: &HeaderMap,
    name: &'static str,
    missing_or_invalid: StatusCode,
) -> Result<String, StatusCode> {
    optional_header(headers, name).ok_or(missing_or_invalid)
}

fn optional_header(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn whatsapp_verify(
    State(state): State<AppState>,
    Query(query): Query<WhatsAppVerifyQuery>,
) -> Result<String, StatusCode> {
    acknowledge_state_shape(&state);

    match (query.verify_token, query.challenge) {
        (Some(verify_token), Some(challenge))
            if verify_token == state.config.whatsapp.verify_token.as_str() =>
        {
            Ok(challenge)
        }
        _ => Err(StatusCode::FORBIDDEN),
    }
}

async fn whatsapp_webhook(State(state): State<AppState>, body: String) -> StatusCode {
    whatsapp_webhook_status(
        process_whatsapp_webhook(&state.store, &body, |sender, reply| {
            let client = state.whatsapp_client.clone();
            async move { client.send_text_reply(&sender, &reply).await }
        })
        .await,
    )
}

#[cfg(test)]
async fn whatsapp_webhook_with_reply_sender<SendReply, SendReplyFuture>(
    State(state): State<AppState>,
    body: String,
    send_reply: SendReply,
) -> StatusCode
where
    SendReply: FnMut(String, String) -> SendReplyFuture,
    SendReplyFuture: Future<Output = Result<(), WhatsAppReplyError>>,
{
    whatsapp_webhook_status(process_whatsapp_webhook(&state.store, &body, send_reply).await)
}

fn whatsapp_webhook_status(result: Result<(), WhatsAppWebhookError>) -> StatusCode {
    match result {
        Ok(()) => StatusCode::OK,
        Err(WhatsAppWebhookError::Payload(error)) => {
            eprintln!("Invalid WhatsApp webhook payload: {error}");
            StatusCode::BAD_REQUEST
        }
        Err(WhatsAppWebhookError::Command(error)) => {
            eprintln!("Failed to update list from WhatsApp webhook: {error}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn trmnl_display(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<trmnl::DisplayResponse>, StatusCode> {
    acknowledge_state_shape(&state);
    let firmware_headers = TrmnlFirmwareHeaders::from_headers(&headers)?;
    firmware_headers.validate_access_token(state.config.trmnl.token.as_str())?;
    let image_url = format!("{}/trmnl/list.png", state.config.public_base_url);

    Ok(Json(trmnl::DisplayResponse::new(image_url, "list.png")))
}

async fn trmnl_image(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    acknowledge_state_shape(&state);
    let firmware_headers = TrmnlFirmwareHeaders::from_headers(&headers)?;
    firmware_headers.validate_access_token(state.config.trmnl.token.as_str())?;

    let entries = state
        .store
        .list_entries()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let png = render_trmnl_list_png(&entries).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(png))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn trmnl_log(State(state): State<AppState>, headers: HeaderMap, body: String) -> StatusCode {
    acknowledge_state_shape(&state);
    let firmware_headers = match TrmnlFirmwareHeaders::from_headers(&headers) {
        Ok(firmware_headers) => firmware_headers,
        Err(status) => return status,
    };
    if let Err(status) = firmware_headers.validate_access_token(state.config.trmnl.token.as_str()) {
        return status;
    }
    if let Some(summary) = firmware_headers.telemetry_summary() {
        println!(
            "TRMNL log headers from {}: {summary}",
            firmware_headers.device_id
        );
    }

    if body.trim().is_empty() {
        return StatusCode::OK;
    }

    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::BAD_REQUEST,
    }
}

fn acknowledge_state_shape(state: &AppState) {
    let _ = (&state.config, &state.store, &state.whatsapp_client);
}

const TRMNL_IMAGE_WIDTH: u32 = 800;
const TRMNL_IMAGE_HEIGHT: u32 = 480;
const TRMNL_MARGIN: u32 = 32;
const FONT_WIDTH: u32 = 3;
const FONT_HEIGHT: u32 = 5;
const FONT_SCALE: u32 = 4;
const FONT_SPACING: u32 = 2;
const LINE_HEIGHT: u32 = FONT_HEIGHT * FONT_SCALE + 9;
const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const LIGHT_GRAY: [u8; 4] = [224, 224, 224, 255];

fn render_trmnl_list_png(entries: &[crate::store::Entry]) -> Result<Vec<u8>, image::ImageError> {
    let mut canvas = Canvas::new(TRMNL_IMAGE_WIDTH, TRMNL_IMAGE_HEIGHT, WHITE);
    let generated_at = generated_timestamp();
    let entry_count = format!("{} entries", entries.len());

    canvas.draw_text("List", TRMNL_MARGIN, 24, 8, BLACK);
    canvas.draw_text(&entry_count, TRMNL_MARGIN, 78, 3, BLACK);
    canvas.draw_horizontal_line(
        TRMNL_MARGIN,
        112,
        TRMNL_IMAGE_WIDTH - TRMNL_MARGIN,
        LIGHT_GRAY,
    );

    let max_chars = chars_per_line(TRMNL_IMAGE_WIDTH - (TRMNL_MARGIN * 2), FONT_SCALE);
    let footer_y = TRMNL_IMAGE_HEIGHT - 42;
    let mut y = 130;

    if entries.is_empty() {
        canvas.draw_text("No entries", TRMNL_MARGIN, y, FONT_SCALE, BLACK);
    } else {
        for (index, entry) in entries.iter().enumerate() {
            if y + LINE_HEIGHT > footer_y {
                break;
            }

            let prefix = format!("{}. ", index + 1);
            let line_width = max_chars.saturating_sub(prefix.chars().count()).max(8);
            let lines = wrap_text(entry.text(), line_width);

            for (line_index, line) in lines.iter().enumerate() {
                if y + LINE_HEIGHT > footer_y {
                    break;
                }

                if line_index == 0 {
                    canvas.draw_text(
                        &format!("{prefix}{line}"),
                        TRMNL_MARGIN,
                        y,
                        FONT_SCALE,
                        BLACK,
                    );
                } else {
                    let indent = text_width(&prefix, FONT_SCALE);
                    canvas.draw_text(line, TRMNL_MARGIN + indent, y, FONT_SCALE, BLACK);
                }
                y += LINE_HEIGHT;
            }
            y += 6;
        }
    }

    canvas.draw_horizontal_line(
        TRMNL_MARGIN,
        footer_y - 14,
        TRMNL_IMAGE_WIDTH - TRMNL_MARGIN,
        LIGHT_GRAY,
    );
    canvas.draw_text(&generated_at, TRMNL_MARGIN, footer_y, 3, BLACK);

    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    encoder.write_image(
        canvas.pixels(),
        TRMNL_IMAGE_WIDTH,
        TRMNL_IMAGE_HEIGHT,
        image::ColorType::Rgba8.into(),
    )?;

    Ok(png)
}

fn generated_timestamp() -> String {
    let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    };

    format!("Generated: {seconds}s since Unix epoch")
}

fn chars_per_line(width: u32, scale: u32) -> usize {
    let glyph_width = FONT_WIDTH * scale + FONT_SPACING;
    usize::try_from(width / glyph_width).map_or(0, |count| count)
}

fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let normalized = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in normalized.split_whitespace() {
        if word.chars().count() > max_chars {
            push_current_line(&mut lines, &mut current);
            push_long_word(&mut lines, word, max_chars);
            continue;
        }

        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word.chars().count() > max_chars {
            push_current_line(&mut lines, &mut current);
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    push_current_line(&mut lines, &mut current);
    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn push_current_line(lines: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        lines.push(std::mem::take(current));
    }
}

fn push_long_word(lines: &mut Vec<String>, word: &str, max_chars: usize) {
    let mut chunk = String::new();
    for character in word.chars() {
        if chunk.chars().count() == max_chars {
            lines.push(std::mem::take(&mut chunk));
        }
        chunk.push(character);
    }
    if !chunk.is_empty() {
        lines.push(chunk);
    }
}

fn text_width(text: &str, scale: u32) -> u32 {
    let glyph_width = FONT_WIDTH * scale + FONT_SPACING;
    let count = u32::try_from(text.chars().count()).map_or(0, |count| count);

    count.saturating_mul(glyph_width)
}

struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32, color: [u8; 4]) -> Self {
        let pixel_count = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .map_or(0, |pixel_count| pixel_count);
        let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));
        for _ in 0..pixel_count {
            pixels.extend_from_slice(&color);
        }

        Self {
            width,
            height,
            pixels,
        }
    }

    fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    fn draw_text(&mut self, text: &str, x: u32, y: u32, scale: u32, color: [u8; 4]) {
        let mut cursor_x = x;
        for character in text.chars() {
            self.draw_glyph(character, cursor_x, y, scale, color);
            cursor_x = cursor_x.saturating_add(FONT_WIDTH * scale + FONT_SPACING);
        }
    }

    fn draw_glyph(&mut self, character: char, x: u32, y: u32, scale: u32, color: [u8; 4]) {
        let bits = glyph_bits(character);
        for row in 0..FONT_HEIGHT {
            for column in 0..FONT_WIDTH {
                let offset = row * FONT_WIDTH + column;
                let mask = 1 << ((FONT_WIDTH * FONT_HEIGHT - 1) - offset);
                if bits & mask != 0 {
                    self.fill_rect(x + column * scale, y + row * scale, scale, scale, color);
                }
            }
        }
    }

    fn draw_horizontal_line(&mut self, x1: u32, y: u32, x2: u32, color: [u8; 4]) {
        let width = x2.saturating_sub(x1);
        self.fill_rect(x1, y, width, 2, color);
    }

    fn fill_rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
        let end_x = x.saturating_add(width).min(self.width);
        let end_y = y.saturating_add(height).min(self.height);
        for pixel_y in y..end_y {
            for pixel_x in x..end_x {
                self.set_pixel(pixel_x, pixel_y, color);
            }
        }
    }

    fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        let Some(index) = pixel_index(self.width, x, y) else {
            return;
        };
        let Some(pixel) = self.pixels.get_mut(index..index + 4) else {
            return;
        };

        pixel.copy_from_slice(&color);
    }
}

fn pixel_index(width: u32, x: u32, y: u32) -> Option<usize> {
    let width = usize::try_from(width).ok()?;
    let x = usize::try_from(x).ok()?;
    let y = usize::try_from(y).ok()?;

    y.checked_mul(width)?.checked_add(x)?.checked_mul(4)
}

fn glyph_bits(character: char) -> u16 {
    match character.to_ascii_uppercase() {
        'A' => 0b010_101_111_101_101,
        'B' => 0b110_101_110_101_110,
        'C' => 0b011_100_100_100_011,
        'D' => 0b110_101_101_101_110,
        'E' => 0b111_100_110_100_111,
        'F' => 0b111_100_110_100_100,
        'G' => 0b011_100_101_101_011,
        'H' => 0b101_101_111_101_101,
        'I' => 0b111_010_010_010_111,
        'J' => 0b001_001_001_101_010,
        'K' => 0b101_101_110_101_101,
        'L' => 0b100_100_100_100_111,
        'M' => 0b101_111_111_101_101,
        'N' => 0b101_111_111_111_101,
        'O' => 0b010_101_101_101_010,
        'P' => 0b110_101_110_100_100,
        'Q' => 0b010_101_101_111_011,
        'R' => 0b110_101_110_101_101,
        'S' => 0b011_100_010_001_110,
        'T' => 0b111_010_010_010_010,
        'U' => 0b101_101_101_101_111,
        'V' => 0b101_101_101_101_010,
        'W' => 0b101_101_111_111_101,
        'X' => 0b101_101_010_101_101,
        'Y' => 0b101_101_010_010_010,
        'Z' => 0b111_001_010_100_111,
        '0' => 0b111_101_101_101_111,
        '1' => 0b010_110_010_010_111,
        '2' => 0b110_001_010_100_111,
        '3' => 0b110_001_010_001_110,
        '4' => 0b101_101_111_001_001,
        '5' => 0b111_100_110_001_110,
        '6' => 0b011_100_110_101_010,
        '7' => 0b111_001_010_010_010,
        '8' => 0b010_101_010_101_010,
        '9' => 0b010_101_011_001_110,
        '.' => 0b000_000_000_000_010,
        ':' => 0b000_010_000_010_000,
        '-' => 0b000_000_111_000_000,
        '/' => 0b001_001_010_100_100,
        '&' => 0b010_101_010_101_011,
        '+' => 0b000_010_111_010_000,
        '#' => 0b101_111_101_111_101,
        '=' => 0b000_111_000_111_000,
        ' ' => 0,
        _ => 0b111_001_010_000_010,
    }
}

async fn process_whatsapp_webhook<SendReply, SendReplyFuture>(
    store: &StoreHandle,
    body: &str,
    mut send_reply: SendReply,
) -> Result<(), WhatsAppWebhookError>
where
    SendReply: FnMut(String, String) -> SendReplyFuture,
    SendReplyFuture: Future<Output = Result<(), WhatsAppReplyError>>,
{
    for message in parse_inbound_text_messages(body)? {
        let command = parse_command(message.text());
        let reply = execute_command(store, command)?;
        println!("{reply}");
        if let Err(error) = send_reply(message.sender().to_owned(), reply).await {
            eprintln!("Failed to send WhatsApp reply: {error}");
        }
    }

    Ok(())
}

#[derive(Debug)]
enum WhatsAppWebhookError {
    Payload(WhatsAppPayloadError),
    Command(CommandExecutionError),
}

impl From<WhatsAppPayloadError> for WhatsAppWebhookError {
    fn from(error: WhatsAppPayloadError) -> Self {
        Self::Payload(error)
    }
}

impl From<CommandExecutionError> for WhatsAppWebhookError {
    fn from(error: CommandExecutionError) -> Self {
        Self::Command(error)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::http::HeaderValue;

    use super::*;
    use crate::config::{AppConfig, SecretString, TrmnlConfig, WhatsAppConfig};

    #[test]
    fn builds_router_with_shared_state() {
        let state = AppState::new_uninitialized(test_config());

        let app = router(state);

        assert!(app.has_routes());
    }

    #[test]
    fn trmnl_firmware_headers_extract_required_device_id() {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));

        let firmware_headers =
            TrmnlFirmwareHeaders::from_headers(&headers).expect("id header should extract");

        assert_eq!(firmware_headers.device_id, "device-123");
    }

    #[test]
    fn trmnl_firmware_headers_reject_missing_or_invalid_device_id() {
        let headers = HeaderMap::new();
        assert_eq!(
            TrmnlFirmwareHeaders::from_headers(&headers),
            Err(StatusCode::BAD_REQUEST)
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "id",
            HeaderValue::from_bytes(b"\xFF").expect("opaque header value should construct"),
        );

        assert_eq!(
            TrmnlFirmwareHeaders::from_headers(&headers),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn trmnl_firmware_headers_extract_and_validate_access_token() {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers.insert("access-token", HeaderValue::from_static("access-secret"));

        let firmware_headers =
            TrmnlFirmwareHeaders::from_headers(&headers).expect("headers should extract");

        assert_eq!(firmware_headers.require_access_token(), Ok("access-secret"));
        assert_eq!(
            firmware_headers.validate_access_token("access-secret"),
            Ok(())
        );
        assert_eq!(
            firmware_headers.validate_access_token("wrong-secret"),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn trmnl_firmware_headers_reject_missing_or_invalid_access_token() {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        assert_eq!(
            TrmnlFirmwareHeaders::from_headers(&headers)
                .expect("id header should extract")
                .require_access_token(),
            Err(StatusCode::FORBIDDEN)
        );

        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers.insert(
            "access-token",
            HeaderValue::from_bytes(b"\xFF").expect("opaque header value should construct"),
        );

        assert_eq!(
            TrmnlFirmwareHeaders::from_headers(&headers)
                .expect("id header should extract")
                .require_access_token(),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn trmnl_firmware_headers_store_optional_telemetry_as_strings() {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers.insert("battery-voltage", HeaderValue::from_static("not-a-number"));
        headers.insert("fw-version", HeaderValue::from_static("1.8.2"));
        headers.insert("rssi", HeaderValue::from_static("-61"));
        headers.insert("refresh-rate", HeaderValue::from_static("900"));
        headers.insert("model", HeaderValue::from_static("og_png"));
        headers.insert("width", HeaderValue::from_static("800"));
        headers.insert("height", HeaderValue::from_static("480"));
        headers.insert("update-source", HeaderValue::from_static("firmware"));
        headers.insert("temperature-profile", HeaderValue::from_static("cold"));
        headers.insert("sensors", HeaderValue::from_static("battery,wifi"));

        let firmware_headers =
            TrmnlFirmwareHeaders::from_headers(&headers).expect("headers should extract");

        assert_eq!(
            firmware_headers.battery_voltage.as_deref(),
            Some("not-a-number")
        );
        assert_eq!(firmware_headers.fw_version.as_deref(), Some("1.8.2"));
        assert_eq!(firmware_headers.rssi.as_deref(), Some("-61"));
        assert_eq!(firmware_headers.refresh_rate.as_deref(), Some("900"));
        assert_eq!(firmware_headers.model.as_deref(), Some("og_png"));
        assert_eq!(firmware_headers.width.as_deref(), Some("800"));
        assert_eq!(firmware_headers.height.as_deref(), Some("480"));
        assert_eq!(firmware_headers.update_source.as_deref(), Some("firmware"));
        assert_eq!(
            firmware_headers.temperature_profile.as_deref(),
            Some("cold")
        );
        assert_eq!(firmware_headers.sensors.as_deref(), Some("battery,wifi"));
    }

    #[test]
    fn trmnl_firmware_headers_ignore_invalid_optional_telemetry_without_panic() {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers.insert(
            "battery-voltage",
            HeaderValue::from_bytes(b"\xFF").expect("opaque header value should construct"),
        );

        let firmware_headers =
            TrmnlFirmwareHeaders::from_headers(&headers).expect("headers should extract");

        assert_eq!(firmware_headers.battery_voltage, None);
    }

    #[tokio::test]
    async fn whatsapp_verification_returns_exact_challenge_for_correct_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            whatsapp_verify(
                State(state),
                Query(WhatsAppVerifyQuery {
                    verify_token: Some("verify-secret".to_owned()),
                    challenge: Some("challenge-body".to_owned()),
                }),
            )
            .await,
            Ok("challenge-body".to_owned())
        );
    }

    #[tokio::test]
    async fn whatsapp_verification_rejects_wrong_or_missing_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            whatsapp_verify(
                State(state.clone()),
                Query(WhatsAppVerifyQuery {
                    verify_token: Some("wrong-secret".to_owned()),
                    challenge: Some("challenge-body".to_owned()),
                }),
            )
            .await,
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            whatsapp_verify(
                State(state),
                Query(WhatsAppVerifyQuery {
                    verify_token: None,
                    challenge: Some("challenge-body".to_owned()),
                }),
            )
            .await,
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn trmnl_display_returns_image_response_for_valid_firmware_headers() {
        let state = AppState::new_uninitialized(test_config());

        let Json(response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("valid firmware headers should return display response");
        let json = serde_json::to_value(response).expect("display response should serialize");

        assert_eq!(json["status"], 0);
        assert_eq!(json["image_url"], "https://example.test/trmnl/list.png");
        assert_eq!(json["filename"], "list.png");
    }

    #[tokio::test]
    async fn trmnl_display_rejects_missing_id() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_display(State(state), trmnl_headers_without_id())
                .await
                .err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn trmnl_display_rejects_missing_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_display(State(state), trmnl_headers_without_access_token())
                .await
                .err(),
            Some(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn trmnl_display_rejects_wrong_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_display(
                State(state),
                trmnl_headers_with_access_token("wrong-secret")
            )
            .await
            .err(),
            Some(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn trmnl_display_rejects_query_token_only_request_shape() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_display(State(state), HeaderMap::new()).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn trmnl_image_returns_png_for_valid_firmware_headers() {
        let database = TestDatabase::new("trmnl_image_png");
        let state = initialized_state(&database);
        state
            .store
            .add_entry("milk")
            .expect("first entry should insert");
        state
            .store
            .add_entry("eggs")
            .expect("second entry should insert");

        let response = trmnl_image(State(state), valid_trmnl_headers())
            .await
            .expect("valid firmware headers should return image response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&header::HeaderValue::from_static("image/png"))
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("image body should read");
        let image = image::load_from_memory(&bytes).expect("body should decode as image");

        assert_eq!(image.width(), 800);
        assert_eq!(image.height(), 480);
    }

    #[tokio::test]
    async fn trmnl_image_returns_png_for_empty_list() {
        let database = TestDatabase::new("trmnl_image_empty");
        let state = initialized_state(&database);

        let response = trmnl_image(State(state), valid_trmnl_headers())
            .await
            .expect("valid firmware headers should return image response");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("image body should read");
        let image = image::load_from_memory(&bytes).expect("body should decode as image");

        assert_eq!(image.width(), 800);
        assert_eq!(image.height(), 480);
    }

    #[tokio::test]
    async fn trmnl_image_handles_long_entries() {
        let database = TestDatabase::new("trmnl_image_long_entry");
        let state = initialized_state(&database);
        state
            .store
            .add_entry("a".repeat(500))
            .expect("long entry should insert");

        let response = trmnl_image(State(state), valid_trmnl_headers())
            .await
            .expect("valid firmware headers should return image response");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("image body should read");
        let image = image::load_from_memory(&bytes).expect("body should decode as image");

        assert_eq!(image.width(), 800);
        assert_eq!(image.height(), 480);
    }

    #[tokio::test]
    async fn trmnl_log_accepts_representative_json_telemetry() {
        let state = AppState::new_uninitialized(test_config());

        let status = trmnl_log(
            State(state),
            valid_trmnl_headers(),
            r#"{
                "device": "trmnl",
                "battery_voltage": 4.12,
                "wifi_signal": -61,
                "refresh_rate": 900,
                "firmware_version": "1.0.0"
            }"#
            .to_owned(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn trmnl_log_does_not_mutate_list_entries() {
        let database = TestDatabase::new("trmnl_log_no_mutation");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");

        let status = trmnl_log(
            State(state.clone()),
            valid_trmnl_headers(),
            r#"{"device":"trmnl","event":"refresh"}"#.to_owned(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let entries = state.store.list_entries().expect("entries should list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "milk");
    }

    #[tokio::test]
    async fn trmnl_log_rejects_invalid_json_without_secret_response_body() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_log(State(state), valid_trmnl_headers(), "{".to_owned()).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn trmnl_log_accepts_empty_body_with_valid_firmware_headers() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_log(State(state), valid_trmnl_headers(), String::new()).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn trmnl_log_rejects_missing_id() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_log(
                State(state),
                trmnl_headers_without_id(),
                r#"{"event":"refresh"}"#.to_owned(),
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn trmnl_log_rejects_missing_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_log(
                State(state),
                trmnl_headers_without_access_token(),
                r#"{"event":"refresh"}"#.to_owned(),
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn trmnl_log_rejects_wrong_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_log(
                State(state),
                trmnl_headers_with_access_token("wrong-secret"),
                r#"{"event":"refresh"}"#.to_owned(),
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn trmnl_image_rejects_missing_id() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(State(state), trmnl_headers_without_id())
                .await
                .err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn trmnl_image_rejects_missing_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(State(state), trmnl_headers_without_access_token())
                .await
                .err(),
            Some(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn trmnl_image_rejects_wrong_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(
                State(state),
                trmnl_headers_with_access_token("wrong-secret")
            )
            .await
            .err(),
            Some(StatusCode::FORBIDDEN)
        );
    }

    #[tokio::test]
    async fn whatsapp_webhook_processes_text_messages_in_payload_order() {
        let database = TestDatabase::new("webhook_processes_messages");
        let state = initialized_state(&database);
        let mut sent_replies = Vec::new();

        let status = whatsapp_webhook_with_reply_sender(
            State(state.clone()),
            multi_message_payload().to_owned(),
            |sender, reply| {
                sent_replies.push((sender, reply));
                async { Ok(()) }
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            sent_replies,
            [
                (
                    "15550000001".to_owned(),
                    "\"milk\" added to list.".to_owned()
                ),
                (
                    "15550000002".to_owned(),
                    "\"eggs\" added to list.".to_owned()
                ),
                (
                    "15550000001".to_owned(),
                    "\"milk\" removed from list.".to_owned()
                ),
                (
                    "15550000002".to_owned(),
                    "\"eggs\" removed from list.".to_owned()
                ),
            ]
        );
        let entries = state.store.list_entries().expect("entries should list");
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn whatsapp_webhook_acknowledges_processed_message_when_reply_fails() {
        let database = TestDatabase::new("webhook_acknowledges_reply_failure");
        let state = initialized_state(&database);
        let mut reply_attempts = 0;

        let status = whatsapp_webhook_with_reply_sender(
            State(state.clone()),
            single_message_payload("15550000001", "Cow").to_owned(),
            |_sender, _reply| {
                reply_attempts += 1;
                async { Err(request_build_reply_error()) }
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(reply_attempts, 1);
        let entries = state.store.list_entries().expect("entries should list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "Cow");
    }

    #[tokio::test]
    async fn whatsapp_webhook_ignores_status_and_non_text_payloads() {
        let database = TestDatabase::new("webhook_ignores_non_text");
        let state = initialized_state(&database);
        let mut sent_replies = Vec::new();

        let status = whatsapp_webhook_with_reply_sender(
            State(state.clone()),
            status_and_non_text_payload().to_owned(),
            |sender, reply| {
                sent_replies.push((sender, reply));
                async { Ok(()) }
            },
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(sent_replies.is_empty());
        assert!(
            state
                .store
                .list_entries()
                .expect("entries should list")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn whatsapp_webhook_rejects_invalid_json_without_secret_response_body() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            whatsapp_webhook(State(state), "{".to_owned()).await,
            StatusCode::BAD_REQUEST
        );
    }

    fn valid_trmnl_headers() -> HeaderMap {
        trmnl_headers_with_access_token("trmnl-secret")
    }

    fn trmnl_headers_with_access_token(access_token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers.insert(
            "access-token",
            HeaderValue::from_str(access_token).expect("test access token should be valid"),
        );
        headers
    }

    fn trmnl_headers_without_id() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("access-token", HeaderValue::from_static("trmnl-secret"));
        headers
    }

    fn trmnl_headers_without_access_token() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("id", HeaderValue::from_static("device-123"));
        headers
    }

    fn test_config() -> AppConfig {
        AppConfig {
            whatsapp: WhatsAppConfig {
                verify_token: SecretString::from_test_value("verify-secret"),
                access_token: SecretString::from_test_value("access-secret"),
                phone_number_id: "phone-number".to_owned(),
            },
            trmnl: TrmnlConfig {
                token: SecretString::from_test_value("trmnl-secret"),
            },
            public_base_url: "https://example.test".to_owned(),
            database_path: PathBuf::from("list.db"),
            bind_addr: "127.0.0.1:3000".to_owned(),
        }
    }

    fn initialized_state(database: &TestDatabase) -> AppState {
        let mut config = test_config();
        config.database_path = database.path().to_path_buf();

        AppState::new(config).expect("app state should initialize")
    }

    fn multi_message_payload() -> &'static str {
        r#"{
            "entry": [
                {
                    "changes": [
                        {
                            "value": {
                                "messages": [
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.first",
                                        "type": "text",
                                        "text": { "body": "milk" }
                                    },
                                    {
                                        "from": "15550000002",
                                        "id": "wamid.second",
                                        "type": "text",
                                        "text": { "body": "eggs" }
                                    },
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.third",
                                        "type": "text",
                                        "text": { "body": "milk" }
                                    },
                                    {
                                        "from": "15550000002",
                                        "id": "wamid.fourth",
                                        "type": "text",
                                        "text": { "body": "eggs" }
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#
    }

    fn single_message_payload(sender: &str, text: &str) -> String {
        format!(
            r#"{{
                "entry": [
                    {{
                        "changes": [
                            {{
                                "value": {{
                                    "messages": [
                                        {{
                                            "from": "{sender}",
                                            "id": "wamid.single",
                                            "text": {{
                                                "body": "{text}"
                                            }},
                                            "type": "text"
                                        }}
                                    ]
                                }}
                            }}
                        ]
                    }}
                ]
            }}"#
        )
    }

    fn request_build_reply_error() -> WhatsAppReplyError {
        let error = reqwest::Client::new()
            .get("http://")
            .build()
            .expect_err("invalid URL should fail request construction");

        WhatsAppReplyError::RequestBuild(error)
    }

    fn status_and_non_text_payload() -> &'static str {
        r#"{
            "entry": [
                {
                    "changes": [
                        {
                            "value": {
                                "statuses": [
                                    {
                                        "id": "wamid.status",
                                        "status": "delivered",
                                        "recipient_id": "15550000001"
                                    }
                                ],
                                "messages": [
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.image",
                                        "type": "image",
                                        "image": { "id": "media-id" }
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#
    }

    struct TestDatabase {
        path: PathBuf,
    }

    impl TestDatabase {
        fn new(name: &str) -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "trmnl-whatsapp-list-http-{name}-{}-{timestamp}.db",
                std::process::id()
            ));

            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
