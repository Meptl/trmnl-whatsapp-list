use crate::config::{AppConfig, MessagingProviderConfig};
use crate::store::{StoreError, StoreHandle};
use crate::telegram::TelegramReplyClient;
use crate::whatsapp::WhatsAppReplyClient;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: StoreHandle,
    pub messaging_client: MessagingClient,
}

#[derive(Clone)]
pub enum MessagingClient {
    WhatsApp(WhatsAppReplyClient),
    Telegram(TelegramReplyClient),
}

impl MessagingClient {
    fn new(config: &MessagingProviderConfig) -> Self {
        match config {
            MessagingProviderConfig::WhatsApp(config) => {
                Self::WhatsApp(WhatsAppReplyClient::new(config))
            }
            MessagingProviderConfig::Telegram(config) => {
                Self::Telegram(TelegramReplyClient::new(config))
            }
        }
    }
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self, StoreError> {
        let store = StoreHandle::new(config.database_path.clone());
        store.initialize()?;

        Ok(Self {
            messaging_client: MessagingClient::new(&config.messaging_provider),
            config,
            store,
        })
    }
}

#[cfg(test)]
impl AppState {
    pub fn new_uninitialized(config: AppConfig) -> Self {
        Self {
            store: StoreHandle::new(config.database_path.clone()),
            messaging_client: MessagingClient::new(&config.messaging_provider),
            config,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::config::{
        MessagingProviderConfig, SecretString, TelegramConfig, TrmnlConfig, WhatsAppConfig,
    };

    #[test]
    fn app_state_contains_whatsapp_client_for_whatsapp_mode() {
        let database_path = temporary_database_path("app_state_whatsapp");
        let config = test_config(
            database_path,
            MessagingProviderConfig::WhatsApp(WhatsAppConfig {
                access_token: SecretString::from_test_value("access-secret"),
                phone_number_id: "phone-number".to_owned(),
            }),
        );

        let state = AppState::new(config.clone()).expect("app state should initialize");
        let cloned_state = state.clone();

        assert_eq!(state.config.public_base_url, "https://example.test");
        assert_eq!(cloned_state.config.database_path, config.database_path);
        assert_eq!(state.store.database_path(), config.database_path);
        assert!(matches!(
            state.messaging_client,
            MessagingClient::WhatsApp(_)
        ));

        fs::remove_file(config.database_path).expect("test database should be removed");
    }

    #[test]
    fn app_state_contains_telegram_client_for_telegram_mode() {
        let database_path = temporary_database_path("app_state_telegram");
        let config = test_config(
            database_path,
            MessagingProviderConfig::Telegram(TelegramConfig {
                bot_token: SecretString::from_test_value("bot-secret"),
            }),
        );

        let state = AppState::new(config.clone()).expect("app state should initialize");

        assert!(matches!(
            state.messaging_client,
            MessagingClient::Telegram(_)
        ));

        fs::remove_file(config.database_path).expect("test database should be removed");
    }

    fn test_config(
        database_path: PathBuf,
        messaging_provider: MessagingProviderConfig,
    ) -> AppConfig {
        AppConfig {
            webhook_key: SecretString::from_test_value("webhook-secret"),
            chat_auth_key: Some(SecretString::from_test_value("chat-secret")),
            messaging_provider,
            trmnl: TrmnlConfig {
                token: SecretString::from_test_value("trmnl-secret"),
            },
            public_base_url: "https://example.test".to_owned(),
            database_path,
            bind_addr: "127.0.0.1:3000".to_owned(),
        }
    }

    fn temporary_database_path(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "trmnl-whatsapp-list-{name}-{}-{timestamp}.db",
            std::process::id()
        ))
    }
}
