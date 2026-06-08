use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::commands::{CommandExecutionError, execute_command, parse_command};
use crate::store::{Entry, StoreHandle};
use crate::whatsapp::{WhatsAppPayloadError, WhatsAppReplyError, parse_inbound_text_messages};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/webhooks/whatsapp",
            get(whatsapp_verify).post(whatsapp_webhook),
        )
        .route("/api/setup", get(trmnl_setup))
        .route("/api/display", get(trmnl_display))
        .route("/trmnl/list.png", get(trmnl_image_route))
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TrmnlDisplayResponse {
    status: u32,
    image_url: String,
    filename: String,
    update_firmware: bool,
    firmware_url: Option<String>,
    refresh_rate: String,
    reset_firmware: bool,
}

#[derive(Debug, Default, Deserialize)]
struct TrmnlImageQuery {
    battery_voltage: Option<String>,
}

impl TrmnlDisplayResponse {
    fn new(image_url: impl Into<String>, filename: impl Into<String>) -> Self {
        Self {
            status: 0,
            image_url: image_url.into(),
            filename: filename.into(),
            update_firmware: false,
            firmware_url: None,
            refresh_rate: "60".to_owned(),
            reset_firmware: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct TrmnlSetupResponse {
    status: u32,
    api_key: String,
    friendly_id: String,
    image_url: String,
    filename: String,
}

impl TrmnlSetupResponse {
    fn new(
        api_key: impl Into<String>,
        friendly_id: impl Into<String>,
        image_url: impl Into<String>,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            status: 200,
            api_key: api_key.into(),
            friendly_id: friendly_id.into(),
            image_url: image_url.into(),
            filename: filename.into(),
        }
    }
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

async fn trmnl_setup(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TrmnlSetupResponse>, StatusCode> {
    acknowledge_state_shape(&state);
    let firmware_headers = match TrmnlFirmwareHeaders::from_headers(&headers) {
        Ok(firmware_headers) => firmware_headers,
        Err(status) => {
            log_trmnl_rejection("/api/setup", status);
            return Err(status);
        }
    };
    let image_url = trmnl_image_url(
        &state.config.public_base_url,
        firmware_headers.battery_voltage.as_deref(),
    );
    let filename =
        trmnl_display_filename(&state.store, firmware_headers.battery_voltage.as_deref())?;
    log_trmnl_success("/api/setup", &firmware_headers, StatusCode::OK);

    Ok(Json(TrmnlSetupResponse::new(
        state.config.trmnl.token.as_str(),
        trmnl_friendly_id(&firmware_headers.device_id),
        image_url,
        filename,
    )))
}

fn trmnl_friendly_id(device_id: &str) -> String {
    let normalized = device_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    let suffix = if normalized.is_empty() {
        trmnl_hashed_suffix(device_id)
    } else {
        normalized
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    };

    format!("trmnl-{suffix}")
}

fn trmnl_hashed_suffix(device_id: &str) -> String {
    let mut hash = 0x811c_9dc5_u32;
    for byte in device_id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }

    format!("{:06x}", hash & 0x00ff_ffff)
}

async fn trmnl_display(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<TrmnlDisplayResponse>, StatusCode> {
    acknowledge_state_shape(&state);
    let firmware_headers = match TrmnlFirmwareHeaders::from_headers(&headers) {
        Ok(firmware_headers) => firmware_headers,
        Err(status) => {
            log_trmnl_rejection("/api/display", status);
            return Err(status);
        }
    };
    if let Err(status) = firmware_headers.validate_access_token(state.config.trmnl.token.as_str()) {
        log_trmnl_success("/api/display", &firmware_headers, status);
        return Err(status);
    }
    let image_url = trmnl_image_url(
        &state.config.public_base_url,
        firmware_headers.battery_voltage.as_deref(),
    );
    let filename =
        trmnl_display_filename(&state.store, firmware_headers.battery_voltage.as_deref())?;
    log_trmnl_success("/api/display", &firmware_headers, StatusCode::OK);

    Ok(Json(TrmnlDisplayResponse::new(image_url, filename)))
}

fn trmnl_image_url(public_base_url: &str, battery_voltage: Option<&str>) -> String {
    let image_url = format!("{public_base_url}/trmnl/list.png");
    match battery_voltage.filter(|voltage| voltage.parse::<f32>().is_ok()) {
        Some(voltage) => format!("{image_url}?battery_voltage={voltage}"),
        None => image_url,
    }
}

fn trmnl_display_filename(
    store: &StoreHandle,
    battery_voltage: Option<&str>,
) -> Result<String, StatusCode> {
    let entries = store
        .list_entries()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(trmnl_list_filename(&entries, battery_voltage))
}

fn trmnl_list_filename(entries: &[Entry], battery_voltage: Option<&str>) -> String {
    let battery = battery_filename_part(battery_voltage);
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for entry in entries {
        hash_filename_bytes(&mut hash, b"id");
        hash_filename_bytes(&mut hash, &entry.id().to_be_bytes());
        hash_filename_bytes(&mut hash, b"text");
        hash_filename_field(&mut hash, entry.text().as_bytes());
        hash_filename_bytes(&mut hash, b"created_at");
        hash_filename_field(&mut hash, entry.created_at().as_bytes());
    }

    format!("list-bat{battery}-{hash:016x}.png")
}

fn battery_filename_part(battery_voltage: Option<&str>) -> String {
    battery_fill_cells(battery_voltage)
        .map_or_else(|| "unknown".to_owned(), |cells| cells.to_string())
}

fn hash_filename_field(hash: &mut u64, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    hash_filename_bytes(hash, &length.to_be_bytes());
    hash_filename_bytes(hash, bytes);
}

fn hash_filename_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

async fn trmnl_image_route(
    State(state): State<AppState>,
    Query(query): Query<TrmnlImageQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    trmnl_image(State(state), query, headers).await
}

async fn trmnl_image(
    State(state): State<AppState>,
    query: TrmnlImageQuery,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    acknowledge_state_shape(&state);
    let firmware_headers = match TrmnlFirmwareHeaders::from_headers(&headers) {
        Ok(firmware_headers) => firmware_headers,
        Err(status) => {
            log_trmnl_rejection("/trmnl/list.png", status);
            return Err(status);
        }
    };
    if let Err(status) = firmware_headers.validate_access_token(state.config.trmnl.token.as_str()) {
        log_trmnl_success("/trmnl/list.png", &firmware_headers, status);
        return Err(status);
    }

    let entries = state
        .store
        .list_entries()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let png = render_trmnl_list_png(&entries, query.battery_voltage.as_deref())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    log_trmnl_success("/trmnl/list.png", &firmware_headers, StatusCode::OK);

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
        Err(status) => {
            log_trmnl_rejection("/api/log", status);
            return status;
        }
    };
    if let Err(status) = firmware_headers.validate_access_token(state.config.trmnl.token.as_str()) {
        log_trmnl_success("/api/log", &firmware_headers, status);
        return status;
    }

    if body.trim().is_empty() {
        log_trmnl_success("/api/log", &firmware_headers, StatusCode::OK);
        return StatusCode::OK;
    }

    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(_) => {
            log_trmnl_success("/api/log", &firmware_headers, StatusCode::OK);
            StatusCode::OK
        }
        Err(_) => {
            log_trmnl_success("/api/log", &firmware_headers, StatusCode::BAD_REQUEST);
            StatusCode::BAD_REQUEST
        }
    }
}

fn log_trmnl_rejection(route: &str, status: StatusCode) {
    println!("TRMNL {route} rejected: http_status={}", status.as_u16());
}

fn log_trmnl_success(route: &str, headers: &TrmnlFirmwareHeaders, status: StatusCode) {
    match headers.telemetry_summary() {
        Some(summary) => println!(
            "TRMNL {route} device_id={} http_status={} {summary}",
            headers.device_id,
            status.as_u16()
        ),
        None => println!(
            "TRMNL {route} device_id={} http_status={}",
            headers.device_id,
            status.as_u16()
        ),
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

fn render_trmnl_list_png(
    entries: &[crate::store::Entry],
    battery_voltage: Option<&str>,
) -> Result<Vec<u8>, image::ImageError> {
    let mut canvas = Canvas::new(TRMNL_IMAGE_WIDTH, TRMNL_IMAGE_HEIGHT, WHITE);

    canvas.draw_text("List", TRMNL_MARGIN, 24, 8, BLACK);
    draw_battery_indicator(&mut canvas, battery_voltage);

    let max_chars = chars_per_line(TRMNL_IMAGE_WIDTH - (TRMNL_MARGIN * 2), FONT_SCALE);
    let footer_y = TRMNL_IMAGE_HEIGHT - TRMNL_MARGIN;
    let mut y = 118;

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

fn draw_battery_indicator(canvas: &mut Canvas, battery_voltage: Option<&str>) {
    let icon_width = 54;
    let icon_height = 24;
    let terminal_width = 6;
    let icon_x = TRMNL_IMAGE_WIDTH - TRMNL_MARGIN - icon_width - terminal_width;
    let icon_y = 34;

    canvas.draw_rect_outline(icon_x, icon_y, icon_width, icon_height, 3, BLACK);
    canvas.fill_rect(
        icon_x + icon_width,
        icon_y + 7,
        terminal_width,
        icon_height - 14,
        BLACK,
    );

    if let Some(cells) = battery_fill_cells(battery_voltage) {
        for cell in 0..cells {
            canvas.fill_rect(
                icon_x + 7 + (cell * 14),
                icon_y + 6,
                10,
                icon_height - 12,
                BLACK,
            );
        }
    } else {
        canvas.draw_text("?", icon_x + 22, icon_y + 4, 3, BLACK);
    }
}

fn battery_fill_cells(battery_voltage: Option<&str>) -> Option<u32> {
    let percent = trmnl_og_battery_percent(battery_voltage)?;

    if percent >= 66.0 {
        Some(3)
    } else if percent >= 33.0 {
        Some(2)
    } else if percent > 1.0 {
        Some(1)
    } else {
        Some(0)
    }
}

fn trmnl_og_battery_percent(battery_voltage: Option<&str>) -> Option<f32> {
    let voltage = battery_voltage?.parse::<f32>().ok()?;
    let percent = (voltage - 3.0) / 0.012;

    if percent >= 88.0 {
        Some(100.0)
    } else if percent >= 85.0 {
        Some(95.0)
    } else if percent >= 83.0 {
        Some(90.0)
    } else if percent >= 10.0 {
        Some(percent)
    } else {
        Some(1.0)
    }
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

    fn draw_rect_outline(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        stroke: u32,
        color: [u8; 4],
    ) {
        self.fill_rect(x, y, width, stroke, color);
        self.fill_rect(x, y + height - stroke, width, stroke, color);
        self.fill_rect(x, y, stroke, height, color);
        self.fill_rect(x + width - stroke, y, stroke, height, color);
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

    use axum::http::{HeaderValue, Method};
    use axum::response::IntoResponse;

    use super::*;
    use crate::config::{AppConfig, SecretString, TrmnlConfig, WhatsAppConfig};

    #[test]
    fn builds_router_with_shared_state() {
        let state = AppState::new_uninitialized(test_config());

        let app = router(state);

        assert!(app.has_routes());
    }

    #[tokio::test]
    async fn trmnl_firmware_flow_accepts_setup_display_image_and_log_requests() {
        let database = TestDatabase::new("trmnl_firmware_flow");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");
        let app = router(state.clone());

        assert!(app.has_routes());

        let device_id = "AA:BB:CC:DD:EE:FF";
        let setup_response = call_trmnl_route_like(
            state.clone(),
            Method::GET,
            "/api/setup",
            trmnl_headers_with_id(device_id),
            String::new(),
        )
        .await;
        let setup_json = response_json(setup_response, StatusCode::OK).await;
        let api_key = setup_json
            .get("api_key")
            .and_then(serde_json::Value::as_str)
            .expect("setup response should include api_key");

        assert_eq!(api_key, "trmnl-secret");

        let firmware_headers = trmnl_headers_with_id_and_access_token(device_id, api_key);
        let display_response = call_trmnl_route_like(
            state.clone(),
            Method::GET,
            "/api/display",
            firmware_headers.clone(),
            String::new(),
        )
        .await;
        let display_json = response_json(display_response, StatusCode::OK).await;
        let image_url = display_json
            .get("image_url")
            .and_then(serde_json::Value::as_str)
            .expect("display response should include image_url");
        let filename = display_json
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .expect("display response should include filename");

        assert_eq!(image_url, "https://example.test/trmnl/list.png");
        assert_firmware_safe_filename(filename);

        assert_returned_image_url_fetches_png(state.clone(), image_url, firmware_headers.clone())
            .await;

        let log_response = call_trmnl_route_like(
            state,
            Method::POST,
            "/api/log",
            firmware_headers,
            r#"{
                "logMessage": "Display refresh completed",
                "deviceStatusStamp": "2026-05-24T16:45:00Z",
                "firmwareVersion": "1.8.2",
                "batteryVoltage": 4.12,
                "rssi": -61,
                "refreshRate": 900
            }"#
            .to_owned(),
        )
        .await;

        assert_eq!(log_response.status(), StatusCode::OK);
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
    async fn trmnl_setup_returns_expected_setup_response_for_device_id() {
        let database = TestDatabase::new("trmnl_setup_response");
        let state = initialized_state(&database);
        let headers = trmnl_headers_with_id("AA:BB:CC:DD:EE:FF");

        let Json(response) = trmnl_setup(State(state), headers)
            .await
            .expect("valid setup headers should return setup response");
        let json = serde_json::to_value(&response).expect("setup response should serialize");
        let serialized =
            serde_json::to_string(&response).expect("setup response should serialize to string");

        assert_eq!(
            json,
            serde_json::json!({
                "status": 200,
                "api_key": "trmnl-secret",
                "friendly_id": "trmnl-ddeeff",
                "image_url": "https://example.test/trmnl/list.png",
                "filename": "list-batunknown-cbf29ce484222325.png",
            })
        );
        assert_eq!(response.api_key, "trmnl-secret");
        assert_firmware_safe_filename(&response.filename);
        assert!(!serialized.contains("verify-secret"));
        assert!(!serialized.contains("access-secret"));
    }

    #[tokio::test]
    async fn trmnl_setup_image_url_fetches_png_with_same_firmware_headers() {
        let database = TestDatabase::new("trmnl_setup_image_url_fetch");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");
        let firmware_headers = valid_trmnl_headers();

        let Json(response) = trmnl_setup(State(state.clone()), firmware_headers.clone())
            .await
            .expect("setup should return image URL");

        assert_returned_image_url_fetches_png(state, &response.image_url, firmware_headers).await;
    }

    #[test]
    fn trmnl_friendly_id_is_deterministic_safe_suffix() {
        let device_id = "AA:BB:CC:DD:EE:FF";

        let friendly_id = trmnl_friendly_id(device_id);

        assert_eq!(friendly_id, "trmnl-ddeeff");
        assert_eq!(friendly_id, trmnl_friendly_id(device_id));
        assert!(!friendly_id.contains(device_id));
    }

    #[tokio::test]
    async fn trmnl_setup_rejects_missing_or_invalid_device_id() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_setup(State(state.clone()), HeaderMap::new())
                .await
                .err(),
            Some(StatusCode::BAD_REQUEST)
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "id",
            HeaderValue::from_bytes(b"\xFF").expect("opaque header value should construct"),
        );

        assert_eq!(
            trmnl_setup(State(state), headers).await.err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn trmnl_display_returns_image_response_for_valid_firmware_headers() {
        let database = TestDatabase::new("trmnl_display_response");
        let state = initialized_state(&database);

        let Json(response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("valid firmware headers should return display response");
        let json = serde_json::to_value(&response).expect("display response should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "status": 0,
                "image_url": "https://example.test/trmnl/list.png",
                "filename": "list-batunknown-cbf29ce484222325.png",
                "update_firmware": false,
                "firmware_url": null,
                "refresh_rate": "60",
                "reset_firmware": false,
            })
        );
        assert_firmware_safe_filename(&response.filename);
    }

    #[tokio::test]
    async fn trmnl_display_image_url_fetches_png_with_same_firmware_headers() {
        let database = TestDatabase::new("trmnl_display_image_url_fetch");
        let state = initialized_state(&database);
        state.store.add_entry("eggs").expect("entry should insert");
        let firmware_headers = valid_trmnl_headers();

        let Json(response) = trmnl_display(State(state.clone()), firmware_headers.clone())
            .await
            .expect("display should return image URL");

        assert_returned_image_url_fetches_png(state, &response.image_url, firmware_headers).await;
    }

    #[tokio::test]
    async fn trmnl_setup_and_display_share_content_filename() {
        let database = TestDatabase::new("trmnl_setup_display_filename");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");

        let Json(setup_response) =
            trmnl_setup(State(state.clone()), trmnl_headers_with_id("device-123"))
                .await
                .expect("setup should return filename");
        let Json(display_response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("display should return filename");

        assert_eq!(setup_response.filename, display_response.filename);
        assert_firmware_safe_filename(&setup_response.filename);
    }

    #[tokio::test]
    async fn repeated_trmnl_display_requests_without_list_changes_keep_filename() {
        let database = TestDatabase::new("trmnl_display_stable_filename");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");

        let Json(first_response) = trmnl_display(State(state.clone()), valid_trmnl_headers())
            .await
            .expect("first display should return filename");
        let Json(second_response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("second display should return filename");

        assert_eq!(first_response.filename, second_response.filename);
        assert_firmware_safe_filename(&first_response.filename);
    }

    #[tokio::test]
    async fn adding_entry_changes_trmnl_display_filename() {
        let database = TestDatabase::new("trmnl_display_add_changes_filename");
        let state = initialized_state(&database);

        let Json(empty_response) = trmnl_display(State(state.clone()), valid_trmnl_headers())
            .await
            .expect("empty display should return filename");
        state.store.add_entry("milk").expect("entry should insert");
        let Json(updated_response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("updated display should return filename");

        assert_ne!(empty_response.filename, updated_response.filename);
        assert_firmware_safe_filename(&updated_response.filename);
    }

    #[tokio::test]
    async fn removing_entry_changes_trmnl_display_filename() {
        let database = TestDatabase::new("trmnl_display_remove_changes_filename");
        let state = initialized_state(&database);
        state.store.add_entry("milk").expect("entry should insert");
        state.store.add_entry("eggs").expect("entry should insert");

        let Json(original_response) = trmnl_display(State(state.clone()), valid_trmnl_headers())
            .await
            .expect("original display should return filename");
        state
            .store
            .remove_entry_by_text("milk")
            .expect("entry should remove");
        let Json(updated_response) = trmnl_display(State(state), valid_trmnl_headers())
            .await
            .expect("updated display should return filename");

        assert_ne!(original_response.filename, updated_response.filename);
        assert_firmware_safe_filename(&updated_response.filename);
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
    async fn trmnl_display_rejects_query_token_only_route_request() {
        let state = AppState::new_uninitialized(test_config());

        let response = call_trmnl_route_like(
            state,
            Method::GET,
            "/api/display?token=trmnl-secret",
            trmnl_headers_with_id("device-123"),
            String::new(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

        let response = trmnl_image(
            State(state),
            TrmnlImageQuery::default(),
            valid_trmnl_headers(),
        )
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

        let response = trmnl_image(
            State(state),
            TrmnlImageQuery::default(),
            valid_trmnl_headers(),
        )
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

        let response = trmnl_image(
            State(state),
            TrmnlImageQuery::default(),
            valid_trmnl_headers(),
        )
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
                "logMessage": "Display refresh failed",
                "deviceStatusStamp": "2026-05-24T16:45:00Z",
                "firmwareVersion": "1.8.2",
                "batteryVoltage": 4.12,
                "rssi": -61,
                "extraFirmwareField": {
                    "refreshRate": 900,
                    "retryCount": 1
                }
            }"#
            .to_owned(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn trmnl_log_accepts_valid_json_without_optional_firmware_fields() {
        let state = AppState::new_uninitialized(test_config());

        let status = trmnl_log(
            State(state),
            valid_trmnl_headers(),
            r#"{"logMessage":"Display refresh completed"}"#.to_owned(),
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
            r#"{"logMessage":"Display refresh completed","deviceStatusStamp":"2026-05-24T16:45:00Z"}"#
                .to_owned(),
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
            trmnl_image(
                State(state),
                TrmnlImageQuery::default(),
                trmnl_headers_without_id(),
            )
            .await
            .err(),
            Some(StatusCode::BAD_REQUEST)
        );
    }

    #[tokio::test]
    async fn trmnl_image_rejects_missing_access_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(
                State(state),
                TrmnlImageQuery::default(),
                trmnl_headers_without_access_token(),
            )
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
                TrmnlImageQuery::default(),
                trmnl_headers_with_access_token("wrong-secret"),
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

    async fn assert_returned_image_url_fetches_png(
        state: AppState,
        image_url: &str,
        firmware_headers: HeaderMap,
    ) {
        let response = fetch_returned_image_url(state, image_url, firmware_headers).await;

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

    fn assert_firmware_safe_filename(filename: &str) {
        assert!(filename.ends_with(".png"));
        assert!(filename.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.')
        }));
    }

    async fn fetch_returned_image_url(
        state: AppState,
        image_url: &str,
        firmware_headers: HeaderMap,
    ) -> Response {
        let url = reqwest::Url::parse(image_url).expect("image URL should parse");

        assert_eq!(url.as_str(), image_url);
        assert_eq!(url.as_str(), "https://example.test/trmnl/list.png");
        assert_eq!(url.query(), None);

        match url.path() {
            "/trmnl/list.png" => {
                call_trmnl_route_like(
                    state,
                    Method::GET,
                    url.path(),
                    firmware_headers,
                    String::new(),
                )
                .await
            }
            path => panic!("unexpected returned image URL path: {path}"),
        }
    }

    async fn response_json(response: Response, expected_status: StatusCode) -> serde_json::Value {
        assert_eq!(response.status(), expected_status);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");

        serde_json::from_slice(&bytes).expect("response body should parse as JSON")
    }

    async fn call_trmnl_route_like(
        state: AppState,
        method: Method,
        target: &str,
        headers: HeaderMap,
        body: String,
    ) -> Response {
        let path = route_path(target);

        if method == Method::GET && path == "/api/setup" {
            return match trmnl_setup(State(state), headers).await {
                Ok(response) => response.into_response(),
                Err(status) => status.into_response(),
            };
        }
        if method == Method::GET && path == "/api/display" {
            return match trmnl_display(State(state), headers).await {
                Ok(response) => response.into_response(),
                Err(status) => status.into_response(),
            };
        }
        if method == Method::GET && path == "/trmnl/list.png" {
            return match trmnl_image(State(state), trmnl_image_query_from_url(target), headers)
                .await
            {
                Ok(response) => response,
                Err(status) => status.into_response(),
            };
        }
        if method == Method::POST && path == "/api/log" {
            return trmnl_log(State(state), headers, body).await.into_response();
        }

        if matches!(
            path,
            "/api/setup" | "/api/display" | "/trmnl/list.png" | "/api/log"
        ) {
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        } else {
            StatusCode::NOT_FOUND.into_response()
        }
    }

    fn route_path(target: &str) -> &str {
        target.split_once('?').map_or(target, |(path, _query)| path)
    }

    fn trmnl_image_query_from_url(target: &str) -> TrmnlImageQuery {
        let Some((_path, query)) = target.split_once('?') else {
            return TrmnlImageQuery::default();
        };
        let battery_voltage = query.split('&').find_map(|pair| {
            pair.strip_prefix("battery_voltage=")
                .map(std::string::ToString::to_string)
        });

        TrmnlImageQuery { battery_voltage }
    }

    fn trmnl_headers_with_id(device_id: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "id",
            HeaderValue::from_str(device_id).expect("test device id should be valid"),
        );
        headers
    }

    fn trmnl_headers_with_access_token(access_token: &str) -> HeaderMap {
        trmnl_headers_with_id_and_access_token("device-123", access_token)
    }

    fn trmnl_headers_with_id_and_access_token(device_id: &str, access_token: &str) -> HeaderMap {
        let mut headers = trmnl_headers_with_id(device_id);
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
