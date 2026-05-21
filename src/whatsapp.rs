use std::error::Error;
use std::fmt;

use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundTextMessage {
    sender: String,
    message_id: String,
    text: String,
}

impl InboundTextMessage {
    fn new(sender: String, message_id: String, text: String) -> Self {
        Self {
            sender,
            message_id,
            text,
        }
    }

    pub fn sender(&self) -> &str {
        &self.sender
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

pub fn parse_inbound_text_messages(
    payload: &str,
) -> Result<Vec<InboundTextMessage>, WhatsAppPayloadError> {
    let payload = serde_json::from_str::<serde_json::Value>(payload)
        .map_err(WhatsAppPayloadError::InvalidJson)?;
    let Ok(payload) = serde_json::from_value::<WhatsAppWebhookPayload>(payload) else {
        return Ok(Vec::new());
    };

    Ok(payload.into_inbound_text_messages())
}

#[derive(Debug)]
pub enum WhatsAppPayloadError {
    InvalidJson(serde_json::Error),
}

impl fmt::Display for WhatsAppPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson(error) => {
                write!(formatter, "invalid WhatsApp webhook JSON: {error}")
            }
        }
    }
}

impl Error for WhatsAppPayloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidJson(error) => Some(error),
        }
    }
}

#[derive(Deserialize)]
struct WhatsAppWebhookPayload {
    entry: Option<Vec<WhatsAppEntry>>,
}

impl WhatsAppWebhookPayload {
    fn into_inbound_text_messages(self) -> Vec<InboundTextMessage> {
        self.entry
            .into_iter()
            .flatten()
            .flat_map(WhatsAppEntry::into_messages)
            .filter_map(WhatsAppMessage::into_inbound_text_message)
            .collect()
    }
}

#[derive(Deserialize)]
struct WhatsAppEntry {
    changes: Option<Vec<WhatsAppChange>>,
}

impl WhatsAppEntry {
    fn into_messages(self) -> impl Iterator<Item = WhatsAppMessage> {
        self.changes
            .into_iter()
            .flatten()
            .filter_map(|change| change.value)
            .flat_map(|value| value.messages.into_iter().flatten())
    }
}

#[derive(Deserialize)]
struct WhatsAppChange {
    value: Option<WhatsAppValue>,
}

#[derive(Deserialize)]
struct WhatsAppValue {
    messages: Option<Vec<WhatsAppMessage>>,
}

#[derive(Deserialize)]
struct WhatsAppMessage {
    from: Option<String>,
    id: Option<String>,
    #[serde(rename = "type")]
    message_type: Option<String>,
    text: Option<WhatsAppText>,
}

impl WhatsAppMessage {
    fn into_inbound_text_message(self) -> Option<InboundTextMessage> {
        if self.message_type.as_deref() != Some("text") {
            return None;
        }

        Some(InboundTextMessage::new(
            self.from?,
            self.id?,
            self.text?.body?,
        ))
    }
}

#[derive(Deserialize)]
struct WhatsAppText {
    body: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_messages_in_payload_order() {
        let payload = r#"{
            "object": "whatsapp_business_account",
            "entry": [
                {
                    "id": "entry-one",
                    "changes": [
                        {
                            "field": "messages",
                            "value": {
                                "messaging_product": "whatsapp",
                                "messages": [
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.first",
                                        "type": "text",
                                        "text": {
                                            "body": "milk"
                                        }
                                    },
                                    {
                                        "from": "15550000002",
                                        "id": "wamid.second",
                                        "type": "text",
                                        "text": {
                                            "body": "eggs"
                                        }
                                    }
                                ]
                            }
                        },
                        {
                            "field": "messages",
                            "value": {
                                "messaging_product": "whatsapp",
                                "messages": [
                                    {
                                        "from": "15550000003",
                                        "id": "wamid.third",
                                        "type": "text",
                                        "text": {
                                            "body": "bread"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                },
                {
                    "id": "entry-two",
                    "changes": [
                        {
                            "field": "messages",
                            "value": {
                                "messaging_product": "whatsapp",
                                "messages": [
                                    {
                                        "from": "15550000004",
                                        "id": "wamid.fourth",
                                        "type": "text",
                                        "text": {
                                            "body": "coffee"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#;

        let messages =
            parse_inbound_text_messages(payload).expect("valid webhook payload should parse");

        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0].sender(), "15550000001");
        assert_eq!(messages[0].message_id(), "wamid.first");
        assert_eq!(messages[0].text(), "milk");
        assert_eq!(messages[1].sender(), "15550000002");
        assert_eq!(messages[1].message_id(), "wamid.second");
        assert_eq!(messages[1].text(), "eggs");
        assert_eq!(messages[2].sender(), "15550000003");
        assert_eq!(messages[2].message_id(), "wamid.third");
        assert_eq!(messages[2].text(), "bread");
        assert_eq!(messages[3].sender(), "15550000004");
        assert_eq!(messages[3].message_id(), "wamid.fourth");
        assert_eq!(messages[3].text(), "coffee");
    }

    #[test]
    fn ignores_non_text_messages() {
        let payload = r#"{
            "entry": [
                {
                    "changes": [
                        {
                            "value": {
                                "messages": [
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.image",
                                        "type": "image",
                                        "image": {
                                            "id": "media-id"
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#;

        let messages =
            parse_inbound_text_messages(payload).expect("valid webhook payload should parse");

        assert!(messages.is_empty());
    }

    #[test]
    fn ignores_status_only_payloads() {
        let payload = r#"{
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
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#;

        let messages =
            parse_inbound_text_messages(payload).expect("valid webhook payload should parse");

        assert!(messages.is_empty());
    }

    #[test]
    fn unsupported_payload_shapes_do_not_panic() {
        let payload = r#"{
            "entry": [
                {
                    "changes": [
                        {},
                        {
                            "value": {
                                "messages": [
                                    {
                                        "type": "text",
                                        "text": {
                                            "body": "missing identifiers"
                                        }
                                    },
                                    {
                                        "from": "15550000001",
                                        "id": "wamid.missing-text",
                                        "type": "text"
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#;

        let messages =
            parse_inbound_text_messages(payload).expect("valid webhook payload should parse");

        assert!(messages.is_empty());
    }

    #[test]
    fn valid_json_with_unsupported_top_level_shape_returns_no_messages() {
        let messages =
            parse_inbound_text_messages("[]").expect("valid unsupported JSON should parse");

        assert!(messages.is_empty());
    }

    #[test]
    fn invalid_json_returns_typed_error() {
        let error = parse_inbound_text_messages("{").expect_err("payload should be invalid");

        assert!(matches!(error, WhatsAppPayloadError::InvalidJson(_)));
    }
}
