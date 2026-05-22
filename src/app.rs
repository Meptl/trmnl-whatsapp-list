use crate::config::AppConfig;
use crate::store::{StoreError, StoreHandle};
use crate::whatsapp::WhatsAppReplyClient;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: StoreHandle,
    pub whatsapp_client: WhatsAppReplyClient,
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self, StoreError> {
        let store = StoreHandle::new(config.database_path.clone());
        store.initialize()?;

        Ok(Self {
            whatsapp_client: WhatsAppReplyClient::new(&config.whatsapp),
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
            whatsapp_client: WhatsAppReplyClient::new(&config.whatsapp),
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
    use crate::config::{SecretString, TrmnlConfig, WhatsAppConfig};

    #[test]
    fn app_state_contains_configuration_and_placeholders() {
        let database_path = temporary_database_path("app_state");
        let config = AppConfig {
            whatsapp: WhatsAppConfig {
                verify_token: SecretString::from_test_value("verify-secret"),
                access_token: SecretString::from_test_value("access-secret"),
                phone_number_id: "phone-number".to_owned(),
            },
            trmnl: TrmnlConfig {
                token: SecretString::from_test_value("trmnl-secret"),
            },
            public_base_url: "https://example.test".to_owned(),
            database_path,
            bind_addr: "127.0.0.1:3000".to_owned(),
        };

        let state = AppState::new(config.clone()).expect("app state should initialize");
        let cloned_state = state.clone();

        assert_eq!(state.config.public_base_url, "https://example.test");
        assert_eq!(cloned_state.config.database_path, config.database_path);
        assert_eq!(state.store.database_path(), config.database_path);
        let _ = state.whatsapp_client;

        fs::remove_file(config.database_path).expect("test database should be removed");
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
