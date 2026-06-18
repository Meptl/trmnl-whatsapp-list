#![forbid(unsafe_code)]

mod app;
#[allow(dead_code)]
mod commands;
mod config;
mod http;
mod messaging;
mod store;
#[allow(dead_code)]
mod telegram;
#[allow(dead_code)]
mod whatsapp;

use axum::Router;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    run().await
}

async fn run() -> Result<(), StartupError> {
    let config = config::AppConfig::from_env()?;
    let bind_addr = config.bind_addr.clone();
    let webhook_mode = webhook_mode_message(&config.messaging_provider);
    let router = build_router_from_config(config)?;
    let listener = TcpListener::bind(&bind_addr).await?;
    let local_addr = listener.local_addr()?;

    println!("Webhook mode: {webhook_mode}");
    println!("Listening on http://{local_addr}");

    axum::serve(listener, router).await?;

    Ok(())
}

fn webhook_mode_message(provider: &config::MessagingProviderConfig) -> &'static str {
    match provider {
        config::MessagingProviderConfig::WhatsApp(_) => "WhatsApp (/webhooks/whatsapp)",
        config::MessagingProviderConfig::Telegram(_) => "Telegram (/webhooks/telegram)",
        config::MessagingProviderConfig::Both { .. } => {
            "WhatsApp (/webhooks/whatsapp), Telegram (/webhooks/telegram)"
        }
    }
}

fn build_router_from_config(config: config::AppConfig) -> Result<Router, StartupError> {
    let state = app::AppState::new(config)?;

    Ok(http::router(state))
}

#[derive(Debug)]
enum StartupError {
    Config(config::ConfigError),
    Io(std::io::Error),
    Store(store::StoreError),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "server I/O error: {error}"),
            Self::Store(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Store(error) => Some(error),
        }
    }
}

impl From<config::ConfigError> for StartupError {
    fn from(error: config::ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<std::io::Error> for StartupError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<store::StoreError> for StartupError {
    fn from(error: store::StoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{
        AppConfig, MessagingProviderConfig, SecretString, TelegramConfig, TrmnlConfig,
        WhatsAppConfig,
    };

    use super::*;

    #[test]
    fn executable_package_metadata_is_declared() {
        assert_eq!(env!("CARGO_PKG_NAME"), "trmnl-whatsapp-list");
    }

    #[test]
    fn webhook_mode_message_names_whatsapp_route() {
        let provider = MessagingProviderConfig::WhatsApp(WhatsAppConfig {
            access_token: SecretString::from_test_value("access-secret"),
            phone_number_id: "phone-number".to_owned(),
        });

        assert_eq!(
            webhook_mode_message(&provider),
            "WhatsApp (/webhooks/whatsapp)"
        );
    }

    #[test]
    fn webhook_mode_message_names_telegram_route() {
        let provider = MessagingProviderConfig::Telegram(TelegramConfig {
            bot_token: SecretString::from_test_value("bot-secret"),
        });

        assert_eq!(
            webhook_mode_message(&provider),
            "Telegram (/webhooks/telegram)"
        );
    }

    #[test]
    fn webhook_mode_message_names_both_routes() {
        let provider = MessagingProviderConfig::Both {
            whatsapp: WhatsAppConfig {
                access_token: SecretString::from_test_value("access-secret"),
                phone_number_id: "phone-number".to_owned(),
            },
            telegram: TelegramConfig {
                bot_token: SecretString::from_test_value("bot-secret"),
            },
        };

        assert_eq!(
            webhook_mode_message(&provider),
            "WhatsApp (/webhooks/whatsapp), Telegram (/webhooks/telegram)"
        );
    }

    #[test]
    fn build_router_from_config_initializes_store() {
        let database_path = temporary_path("startup-initializes-store");
        let config = test_config(database_path.clone());

        let router = build_router_from_config(config).expect("startup composition should succeed");

        assert!(router.has_routes());
        assert!(database_path.exists());

        fs::remove_file(database_path).expect("test database should be removed");
    }

    #[test]
    fn build_router_from_config_fails_on_store_initialization_error() {
        let directory_path = temporary_path("startup-store-error");
        fs::create_dir(&directory_path).expect("test directory should be created");
        let config = test_config(directory_path.clone());

        let error = build_router_from_config(config).expect_err("startup should fail");

        assert!(matches!(error, StartupError::Store(_)));

        fs::remove_dir(directory_path).expect("test directory should be removed");
    }

    fn test_config(database_path: PathBuf) -> AppConfig {
        AppConfig {
            webhook_key: SecretString::from_test_value("webhook-secret"),
            chat_auth_key: Some(SecretString::from_test_value("chat-secret")),
            messaging_provider: MessagingProviderConfig::WhatsApp(WhatsAppConfig {
                access_token: SecretString::from_test_value("access-secret"),
                phone_number_id: "phone-number".to_owned(),
            }),
            trmnl: TrmnlConfig {
                token: SecretString::from_test_value("trmnl-secret"),
            },
            public_base_url: "https://example.test".to_owned(),
            database_path,
            bind_addr: "127.0.0.1:0".to_owned(),
        }
    }

    fn temporary_path(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "trmnl-whatsapp-list-{name}-{}-{timestamp}",
            std::process::id()
        ))
    }
}
