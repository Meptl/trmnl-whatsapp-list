use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
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

#[derive(Deserialize)]
struct TrmnlTokenQuery {
    token: Option<String>,
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
            let _ = error;
            StatusCode::BAD_REQUEST
        }
        Err(WhatsAppWebhookError::Command(error)) => {
            let _ = error;
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(WhatsAppWebhookError::Reply(error)) => {
            let _ = error;
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn trmnl_display(
    State(state): State<AppState>,
    Query(query): Query<TrmnlTokenQuery>,
) -> Result<Json<trmnl::DisplayResponse>, StatusCode> {
    acknowledge_state_shape(&state);
    let token = validate_trmnl_token(&state, query)?;
    let encoded_token = percent_encode_query_component(&token);
    let image_url = format!(
        "{}/trmnl/list.png?token={encoded_token}",
        state.config.public_base_url
    );

    Ok(Json(trmnl::DisplayResponse::new(image_url, "list.png")))
}

async fn trmnl_image(
    State(state): State<AppState>,
    Query(query): Query<TrmnlTokenQuery>,
) -> Result<StatusCode, StatusCode> {
    acknowledge_state_shape(&state);
    validate_trmnl_token(&state, query)?;

    Ok(StatusCode::NOT_IMPLEMENTED)
}

async fn trmnl_log(State(state): State<AppState>) -> StatusCode {
    acknowledge_state_shape(&state);

    StatusCode::NOT_IMPLEMENTED
}

fn acknowledge_state_shape(state: &AppState) {
    let _ = (&state.config, &state.store, &state.whatsapp_client);
}

fn validate_trmnl_token(state: &AppState, query: TrmnlTokenQuery) -> Result<String, StatusCode> {
    match query.token {
        Some(token) if token == state.config.trmnl.token.as_str() => Ok(token),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

fn percent_encode_query_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }

    encoded
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
        send_reply(message.sender().to_owned(), reply).await?;
    }

    Ok(())
}

#[derive(Debug)]
enum WhatsAppWebhookError {
    Payload(WhatsAppPayloadError),
    Command(CommandExecutionError),
    Reply(WhatsAppReplyError),
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

impl From<WhatsAppReplyError> for WhatsAppWebhookError {
    fn from(error: WhatsAppReplyError) -> Self {
        Self::Reply(error)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::commands::Command;
    use crate::config::{AppConfig, SecretString, TrmnlConfig, WhatsAppConfig};

    #[test]
    fn builds_router_with_shared_state() {
        let state = AppState::new_uninitialized(test_config());

        let app = router(state);

        assert!(app.has_routes());
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
    async fn trmnl_display_returns_image_response_for_valid_token() {
        let state = AppState::new_uninitialized(test_config());

        let Json(response) = trmnl_display(
            State(state),
            Query(TrmnlTokenQuery {
                token: Some("trmnl-secret".to_owned()),
            }),
        )
        .await
        .expect("valid token should return display response");
        let json = serde_json::to_value(response).expect("display response should serialize");

        assert_eq!(json["status"], 0);
        assert_eq!(
            json["image_url"],
            "https://example.test/trmnl/list.png?token=trmnl-secret"
        );
        assert_eq!(json["filename"], "list.png");
    }

    #[tokio::test]
    async fn trmnl_display_percent_encodes_reserved_token_in_image_url() {
        let token = "trmnl&secret+value#part=1";
        let mut config = test_config();
        config.trmnl.token = SecretString::from_test_value(token);
        let state = AppState::new_uninitialized(config);

        let Json(response) = trmnl_display(
            State(state),
            Query(TrmnlTokenQuery {
                token: Some(token.to_owned()),
            }),
        )
        .await
        .expect("valid token should return display response");
        let json = serde_json::to_value(response).expect("display response should serialize");

        assert_eq!(
            json["image_url"],
            "https://example.test/trmnl/list.png?token=trmnl%26secret%2Bvalue%23part%3D1"
        );
    }

    #[tokio::test]
    async fn placeholder_handlers_do_not_return_success() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(
                State(state.clone()),
                Query(TrmnlTokenQuery {
                    token: Some("trmnl-secret".to_owned()),
                }),
            )
            .await,
            Ok(StatusCode::NOT_IMPLEMENTED)
        );
        assert_eq!(trmnl_log(State(state)).await, StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn trmnl_display_rejects_wrong_or_missing_token() {
        let state = AppState::new_uninitialized(test_config());

        assert!(matches!(
            trmnl_display(
                State(state.clone()),
                Query(TrmnlTokenQuery {
                    token: Some("wrong-secret".to_owned()),
                }),
            )
            .await,
            Err(StatusCode::FORBIDDEN)
        ));
        assert!(matches!(
            trmnl_display(State(state), Query(TrmnlTokenQuery { token: None })).await,
            Err(StatusCode::FORBIDDEN)
        ));
    }

    #[tokio::test]
    async fn trmnl_image_rejects_wrong_or_missing_token() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            trmnl_image(
                State(state.clone()),
                Query(TrmnlTokenQuery {
                    token: Some("wrong-secret".to_owned()),
                }),
            )
            .await,
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            trmnl_image(State(state), Query(TrmnlTokenQuery { token: None })).await,
            Err(StatusCode::FORBIDDEN)
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
                ("15550000001".to_owned(), "Added: milk".to_owned()),
                ("15550000002".to_owned(), "Added: eggs".to_owned()),
                ("15550000001".to_owned(), "1. milk\n2. eggs".to_owned()),
                ("15550000002".to_owned(), "Removed: milk".to_owned()),
            ]
        );
        assert_eq!(
            execute_command(&state.store, Command::ListEntries)
                .expect("list command should execute"),
            "1. eggs"
        );
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
        assert_eq!(
            execute_command(&state.store, Command::ListEntries)
                .expect("list command should execute"),
            "The list is empty."
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
                                        "text": { "body": "list" }
                                    },
                                    {
                                        "from": "15550000002",
                                        "id": "wamid.fourth",
                                        "type": "text",
                                        "text": { "body": "remove 1" }
                                    }
                                ]
                            }
                        }
                    ]
                }
            ]
        }"#
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
