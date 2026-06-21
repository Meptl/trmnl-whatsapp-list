use crate::calendar::GoogleCalendarClient;
use crate::config::{AppConfig, MessagingProviderConfig};
use crate::store::{StoreError, StoreHandle};
use crate::telegram::TelegramReplyClient;
use crate::whatsapp::WhatsAppReplyClient;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: StoreHandle,
    pub messaging_client: MessagingClient,
    pub calendar_client: GoogleCalendarClient,
}

#[derive(Clone)]
pub enum MessagingClient {
    WhatsApp(WhatsAppReplyClient),
    Telegram(TelegramReplyClient),
    Both {
        whatsapp: WhatsAppReplyClient,
        telegram: TelegramReplyClient,
    },
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
            MessagingProviderConfig::Both { whatsapp, telegram } => Self::Both {
                whatsapp: WhatsAppReplyClient::new(whatsapp),
                telegram: TelegramReplyClient::new(telegram),
            },
        }
    }

    pub fn whatsapp(&self) -> Option<WhatsAppReplyClient> {
        match self {
            Self::WhatsApp(client) => Some(client.clone()),
            Self::Both { whatsapp, .. } => Some(whatsapp.clone()),
            Self::Telegram(_) => None,
        }
    }

    pub fn telegram(&self) -> Option<TelegramReplyClient> {
        match self {
            Self::Telegram(client) => Some(client.clone()),
            Self::Both { telegram, .. } => Some(telegram.clone()),
            Self::WhatsApp(_) => None,
        }
    }
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self, StoreError> {
        let store = StoreHandle::new(config.database_path.clone());
        store.initialize()?;

        #[cfg(test)]
        let calendar_client = GoogleCalendarClient::test_unavailable();
        #[cfg(not(test))]
        let calendar_client = GoogleCalendarClient::new(&config.google_calendar);

        Ok(Self {
            messaging_client: MessagingClient::new(&config.messaging_provider),
            calendar_client,
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
            calendar_client: GoogleCalendarClient::test_unavailable(),
            config,
        }
    }

    pub fn with_calendar_client_for_tests(
        config: AppConfig,
        calendar_client: GoogleCalendarClient,
    ) -> Self {
        Self {
            store: StoreHandle::new(config.database_path.clone()),
            messaging_client: MessagingClient::new(&config.messaging_provider),
            calendar_client,
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
        GoogleCalendarConfig, MessagingProviderConfig, SecretString, TelegramConfig, TrmnlConfig,
        WhatsAppConfig,
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

    #[test]
    fn app_state_contains_both_clients_for_both_provider_mode() {
        let database_path = temporary_database_path("app_state_both");
        let config = test_config(
            database_path,
            MessagingProviderConfig::Both {
                whatsapp: WhatsAppConfig {
                    access_token: SecretString::from_test_value("access-secret"),
                    phone_number_id: "phone-number".to_owned(),
                },
                telegram: TelegramConfig {
                    bot_token: SecretString::from_test_value("bot-secret"),
                },
            },
        );

        let state = AppState::new(config.clone()).expect("app state should initialize");

        assert!(matches!(
            state.messaging_client,
            MessagingClient::Both { .. }
        ));
        assert!(state.messaging_client.whatsapp().is_some());
        assert!(state.messaging_client.telegram().is_some());

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
            google_calendar: GoogleCalendarConfig {
                client_id: "google-client-id".to_owned(),
                client_secret: SecretString::from_test_value("google-client-secret"),
                refresh_token: SecretString::from_test_value("google-refresh-token"),
            },
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
