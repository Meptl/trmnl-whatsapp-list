use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: StoreHandle,
    pub whatsapp_client: WhatsAppClientHandle,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            store: StoreHandle,
            whatsapp_client: WhatsAppClientHandle,
        }
    }
}

#[derive(Clone)]
pub struct StoreHandle;

#[derive(Clone)]
pub struct WhatsAppClientHandle;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::{SecretString, TrmnlConfig, WhatsAppConfig};

    #[test]
    fn app_state_contains_configuration_and_placeholders() {
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
            database_path: PathBuf::from("list.db"),
            bind_addr: "127.0.0.1:3000".to_owned(),
        };

        let state = AppState::new(config.clone());
        let cloned_state = state.clone();

        assert_eq!(state.config.public_base_url, "https://example.test");
        assert_eq!(cloned_state.config.database_path, config.database_path);
        let _ = state.store;
        let _ = state.whatsapp_client;
    }
}
