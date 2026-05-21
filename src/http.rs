use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde::Deserialize;

use crate::app::AppState;

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

async fn whatsapp_webhook(State(state): State<AppState>) -> StatusCode {
    acknowledge_state_shape(&state);

    StatusCode::NOT_IMPLEMENTED
}

async fn trmnl_display(State(state): State<AppState>) -> StatusCode {
    acknowledge_state_shape(&state);

    StatusCode::NOT_IMPLEMENTED
}

async fn trmnl_image(State(state): State<AppState>) -> StatusCode {
    acknowledge_state_shape(&state);

    StatusCode::NOT_IMPLEMENTED
}

async fn trmnl_log(State(state): State<AppState>) -> StatusCode {
    acknowledge_state_shape(&state);

    StatusCode::NOT_IMPLEMENTED
}

fn acknowledge_state_shape(state: &AppState) {
    let _ = (&state.config, &state.store, &state.whatsapp_client);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
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
    async fn placeholder_handlers_do_not_return_success() {
        let state = AppState::new_uninitialized(test_config());

        assert_eq!(
            whatsapp_webhook(State(state.clone())).await,
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(
            trmnl_display(State(state.clone())).await,
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(
            trmnl_image(State(state.clone())).await,
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(trmnl_log(State(state)).await, StatusCode::NOT_IMPLEMENTED);
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
}
