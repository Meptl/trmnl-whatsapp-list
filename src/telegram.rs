use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::config::{SecretString, TelegramConfig};
use crate::messaging::InboundTextMessage;

const TELEGRAM_API_BASE_URL: &str = "https://api.telegram.org";

pub fn parse_inbound_text_messages(
    payload: &str,
) -> Result<Vec<InboundTextMessage>, TelegramPayloadError> {
    let payload = serde_json::from_str::<serde_json::Value>(payload)
        .map_err(TelegramPayloadError::InvalidJson)?;
    let Ok(update) = serde_json::from_value::<TelegramUpdate>(payload) else {
        return Ok(Vec::new());
    };

    Ok(update.into_inbound_text_message().into_iter().collect())
}

#[derive(Clone)]
pub struct TelegramReplyClient {
    http_client: reqwest::Client,
    bot_token: SecretString,
}

impl TelegramReplyClient {
    pub fn new(config: &TelegramConfig) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            bot_token: config.bot_token.clone(),
        }
    }

    pub fn build_text_reply_request(
        &self,
        chat_id: &str,
        reply_text: &str,
    ) -> Result<reqwest::Request, TelegramReplyError> {
        self.http_client
            .post(self.send_message_url())
            .json(&TextReplyPayload::new(chat_id, reply_text))
            .build()
            .map_err(TelegramReplyError::RequestBuild)
    }

    pub async fn send_text_reply(
        &self,
        chat_id: &str,
        reply_text: &str,
    ) -> Result<(), TelegramReplyError> {
        let request = self.build_text_reply_request(chat_id, reply_text)?;

        let response = self
            .http_client
            .execute(request)
            .await
            .map_err(TelegramReplyError::Send)?;
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.map_err(TelegramReplyError::Send)?;
            return Err(TelegramReplyError::HttpStatus { status, body });
        }

        Ok(())
    }

    fn send_message_url(&self) -> String {
        format!(
            "{TELEGRAM_API_BASE_URL}/bot{}/sendMessage",
            self.bot_token.as_str()
        )
    }
}

impl fmt::Debug for TelegramReplyClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelegramReplyClient")
            .finish_non_exhaustive()
    }
}

pub enum TelegramReplyError {
    RequestBuild(reqwest::Error),
    Send(reqwest::Error),
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },
}

impl fmt::Debug for TelegramReplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestBuild(_) => formatter.write_str("TelegramReplyError::RequestBuild"),
            Self::Send(_) => formatter.write_str("TelegramReplyError::Send"),
            Self::HttpStatus { status, body } => formatter
                .debug_struct("TelegramReplyError::HttpStatus")
                .field("status", status)
                .field("body", body)
                .finish(),
        }
    }
}

impl fmt::Display for TelegramReplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestBuild(_) => write!(formatter, "failed to build Telegram reply request"),
            Self::Send(_) => write!(formatter, "failed to send Telegram reply request"),
            Self::HttpStatus { status, body } => write!(
                formatter,
                "Telegram reply request returned an unsuccessful status: HTTP {status}: {body}"
            ),
        }
    }
}

impl Error for TelegramReplyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RequestBuild(error) | Self::Send(error) => Some(error),
            Self::HttpStatus { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum TelegramPayloadError {
    InvalidJson(serde_json::Error),
}

impl fmt::Display for TelegramPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => write!(formatter, "invalid Telegram webhook JSON: {error}"),
        }
    }
}

impl Error for TelegramPayloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
        }
    }
}

#[derive(Deserialize)]
struct TelegramUpdate {
    message: Option<TelegramMessage>,
}

impl TelegramUpdate {
    fn into_inbound_text_message(self) -> Option<InboundTextMessage> {
        self.message?.into_inbound_text_message()
    }
}

#[derive(Deserialize)]
struct TelegramMessage {
    message_id: Option<i64>,
    chat: Option<TelegramChat>,
    text: Option<String>,
}

impl TelegramMessage {
    fn into_inbound_text_message(self) -> Option<InboundTextMessage> {
        Some(InboundTextMessage::new(
            self.chat?.id?.to_string(),
            self.message_id?.to_string(),
            self.text?,
        ))
    }
}

#[derive(Deserialize)]
struct TelegramChat {
    id: Option<i64>,
}

#[derive(Serialize)]
struct TextReplyPayload<'a> {
    chat_id: &'a str,
    text: &'a str,
}

impl<'a> TextReplyPayload<'a> {
    fn new(chat_id: &'a str, text: &'a str) -> Self {
        Self { chat_id, text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SecretString, TelegramConfig};

    #[test]
    fn extracts_normal_message_text() {
        let payload = r#"{
            "update_id": 1000,
            "message": {
                "message_id": 42,
                "chat": { "id": -12345, "type": "group" },
                "text": "milk"
            }
        }"#;

        let messages = parse_inbound_text_messages(payload).expect("payload should parse");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].reply_target(), "-12345");
        assert_eq!(messages[0].message_id(), "42");
        assert_eq!(messages[0].text(), "milk");
    }

    #[test]
    fn ignores_unsupported_updates() {
        for payload in [
            r#"{"update_id":1000,"edited_message":{"message_id":1,"text":"milk"}}"#,
            r#"{"update_id":1000,"channel_post":{"message_id":1,"text":"milk"}}"#,
            r#"{"update_id":1000,"message":{"message_id":1,"chat":{"id":1},"photo":[]}}"#,
            r#"{"update_id":1000,"message":{"message_id":1,"text":"missing chat"}}"#,
            r#"[]"#,
        ] {
            let messages = parse_inbound_text_messages(payload).expect("payload should parse");

            assert!(messages.is_empty());
        }
    }

    #[test]
    fn invalid_json_returns_typed_error() {
        let error = parse_inbound_text_messages("{").expect_err("payload should be invalid");

        assert!(matches!(error, TelegramPayloadError::InvalidJson(_)));
    }

    #[test]
    fn builds_telegram_send_message_request_without_network() {
        let client = TelegramReplyClient::new(&test_telegram_config());

        let request = client
            .build_text_reply_request("-12345", "Added milk")
            .expect("request should build");

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(
            request.url().as_str(),
            "https://api.telegram.org/botbot-secret/sendMessage"
        );

        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("JSON request body should be buffered");
        let payload: serde_json::Value =
            serde_json::from_slice(body).expect("request body should be JSON");

        assert_eq!(payload["chat_id"], "-12345");
        assert_eq!(payload["text"], "Added milk");
    }

    #[test]
    fn debug_output_does_not_include_bot_token() {
        let client = TelegramReplyClient::new(&test_telegram_config());

        let debug_output = format!("{client:?}");

        assert!(!debug_output.contains("bot-secret"));
    }

    fn test_telegram_config() -> TelegramConfig {
        TelegramConfig {
            bot_token: SecretString::from_test_value("bot-secret"),
        }
    }
}
