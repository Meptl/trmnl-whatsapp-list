use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

pub const WEBHOOK_KEY: &str = "WEBHOOK_KEY";
pub const WHATSAPP_ACCESS_TOKEN: &str = "WHATSAPP_ACCESS_TOKEN";
pub const WHATSAPP_PHONE_NUMBER_ID: &str = "WHATSAPP_PHONE_NUMBER_ID";
pub const TELEGRAM_BOT_TOKEN: &str = "TELEGRAM_BOT_TOKEN";
pub const GOOGLE_CALENDAR_CLIENT_ID: &str = "GOOGLE_CALENDAR_CLIENT_ID";
pub const GOOGLE_CALENDAR_CLIENT_SECRET: &str = "GOOGLE_CALENDAR_CLIENT_SECRET";
pub const GOOGLE_CALENDAR_REFRESH_TOKEN: &str = "GOOGLE_CALENDAR_REFRESH_TOKEN";
pub const CHAT_AUTH_KEY: &str = "CHAT_AUTH_KEY";
const TRMNL_TOKEN: &str = "TRMNL_TOKEN";
const PUBLIC_BASE_URL: &str = "PUBLIC_BASE_URL";
const DATABASE_PATH: &str = "DATABASE_PATH";
const BIND_ADDR: &str = "BIND_ADDR";

const DEFAULT_DATABASE_PATH: &str = "list.db";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

#[derive(Clone)]
pub struct AppConfig {
    pub webhook_key: SecretString,
    pub chat_auth_key: Option<SecretString>,
    pub messaging_provider: MessagingProviderConfig,
    pub google_calendar: GoogleCalendarConfig,
    pub trmnl: TrmnlConfig,
    pub public_base_url: String,
    pub database_path: PathBuf,
    pub bind_addr: String,
}

#[derive(Clone)]
pub enum MessagingProviderConfig {
    WhatsApp(WhatsAppConfig),
    Telegram(TelegramConfig),
    Both {
        whatsapp: WhatsAppConfig,
        telegram: TelegramConfig,
    },
}

impl MessagingProviderConfig {
    pub fn has_whatsapp(&self) -> bool {
        matches!(self, Self::WhatsApp(_) | Self::Both { .. })
    }

    pub fn has_telegram(&self) -> bool {
        matches!(self, Self::Telegram(_) | Self::Both { .. })
    }
}

#[derive(Clone)]
pub struct WhatsAppConfig {
    pub access_token: SecretString,
    pub phone_number_id: String,
}

#[derive(Clone)]
pub struct TelegramConfig {
    pub bot_token: SecretString,
}

#[derive(Clone)]
pub struct GoogleCalendarConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    pub refresh_token: SecretString,
}

#[derive(Clone)]
pub struct TrmnlConfig {
    pub token: SecretString,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretString(String);

#[allow(dead_code)]
impl SecretString {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub fn from_test_value(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("webhook_key", &self.webhook_key)
            .field("chat_auth_key", &self.chat_auth_key)
            .field("messaging_provider", &self.messaging_provider)
            .field("google_calendar", &self.google_calendar)
            .field("trmnl", &self.trmnl)
            .field("public_base_url", &self.public_base_url)
            .field("database_path", &self.database_path)
            .field("bind_addr", &self.bind_addr)
            .finish()
    }
}

impl fmt::Debug for MessagingProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhatsApp(config) => formatter
                .debug_tuple("MessagingProviderConfig::WhatsApp")
                .field(config)
                .finish(),
            Self::Telegram(config) => formatter
                .debug_tuple("MessagingProviderConfig::Telegram")
                .field(config)
                .finish(),
            Self::Both { whatsapp, telegram } => formatter
                .debug_struct("MessagingProviderConfig::Both")
                .field("whatsapp", whatsapp)
                .field("telegram", telegram)
                .finish(),
        }
    }
}

impl fmt::Debug for WhatsAppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WhatsAppConfig")
            .field("access_token", &self.access_token)
            .field("phone_number_id", &self.phone_number_id)
            .finish()
    }
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TelegramConfig")
            .field("bot_token", &self.bot_token)
            .finish()
    }
}

impl fmt::Debug for GoogleCalendarConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoogleCalendarConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret)
            .field("refresh_token", &self.refresh_token)
            .finish()
    }
}

impl fmt::Debug for TrmnlConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrmnlConfig")
            .field("token", &self.token)
            .finish()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConfigError {
    MissingRequiredVariable { variable: &'static str },
    InvalidUnicode { variable: String },
    MissingMessagingProvider,
    IncompleteMessagingProvider { provider: &'static str },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredVariable { variable } => {
                write!(
                    formatter,
                    "missing required environment variable {variable}"
                )
            }
            Self::InvalidUnicode { variable } => {
                write!(
                    formatter,
                    "environment variable {variable} contains invalid unicode"
                )
            }
            Self::MissingMessagingProvider => write!(
                formatter,
                "missing messaging provider configuration for WhatsApp or Telegram"
            ),

            Self::IncompleteMessagingProvider { provider } => {
                write!(
                    formatter,
                    "incomplete {provider} messaging provider configuration"
                )
            }
        }
    }
}

impl Error for ConfigError {}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_pairs(env::vars_os())
    }

    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>,
    ) -> Result<Self, ConfigError> {
        let source = EnvSource::new(pairs)?;

        Ok(Self {
            webhook_key: SecretString(source.required(WEBHOOK_KEY)?),
            chat_auth_key: source.optional(CHAT_AUTH_KEY)?.map(SecretString),
            messaging_provider: messaging_provider_from_source(&source)?,
            google_calendar: google_calendar_from_source(&source)?,
            trmnl: TrmnlConfig {
                token: SecretString(source.required(TRMNL_TOKEN)?),
            },
            public_base_url: source.required(PUBLIC_BASE_URL)?,
            database_path: PathBuf::from(
                source
                    .optional(DATABASE_PATH)?
                    .unwrap_or(DEFAULT_DATABASE_PATH.to_owned()),
            ),
            bind_addr: source
                .optional(BIND_ADDR)?
                .unwrap_or(DEFAULT_BIND_ADDR.to_owned()),
        })
    }
}

fn google_calendar_from_source(source: &EnvSource) -> Result<GoogleCalendarConfig, ConfigError> {
    Ok(GoogleCalendarConfig {
        client_id: source.required(GOOGLE_CALENDAR_CLIENT_ID)?,
        client_secret: SecretString(source.required(GOOGLE_CALENDAR_CLIENT_SECRET)?),
        refresh_token: SecretString(source.required(GOOGLE_CALENDAR_REFRESH_TOKEN)?),
    })
}

fn messaging_provider_from_source(
    source: &EnvSource,
) -> Result<MessagingProviderConfig, ConfigError> {
    let whatsapp_access_token = source.optional(WHATSAPP_ACCESS_TOKEN)?;
    let whatsapp_phone_number_id = source.optional(WHATSAPP_PHONE_NUMBER_ID)?;
    let telegram_bot_token = source.optional(TELEGRAM_BOT_TOKEN)?;

    let telegram = telegram_bot_token.map(|bot_token| TelegramConfig {
        bot_token: SecretString(bot_token),
    });
    let whatsapp = match (whatsapp_access_token, whatsapp_phone_number_id) {
        (Some(access_token), Some(phone_number_id)) => Some(WhatsAppConfig {
            access_token: SecretString(access_token),
            phone_number_id,
        }),
        (None, None) => None,
        _ if telegram.is_some() => None,
        _ => {
            return Err(ConfigError::IncompleteMessagingProvider {
                provider: "WhatsApp",
            });
        }
    };

    match (whatsapp, telegram) {
        (Some(whatsapp), Some(telegram)) => {
            Ok(MessagingProviderConfig::Both { whatsapp, telegram })
        }
        (Some(whatsapp), None) => Ok(MessagingProviderConfig::WhatsApp(whatsapp)),
        (None, Some(telegram)) => Ok(MessagingProviderConfig::Telegram(telegram)),
        (None, None) => Err(ConfigError::MissingMessagingProvider),
    }
}

struct EnvSource {
    pairs: Vec<(String, String)>,
}

impl EnvSource {
    fn new(
        pairs: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>,
    ) -> Result<Self, ConfigError> {
        let pairs = pairs
            .into_iter()
            .map(|(key, value)| {
                let key = key.into();
                let variable = key.to_string_lossy().into_owned();
                let key = into_string(key, variable.clone())?;
                let value = into_string(value.into(), variable)?;

                Ok((key, value))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { pairs })
    }

    fn required(&self, variable: &'static str) -> Result<String, ConfigError> {
        self.optional(variable)?
            .ok_or(ConfigError::MissingRequiredVariable { variable })
    }

    fn optional(&self, variable: &str) -> Result<Option<String>, ConfigError> {
        Ok(self
            .pairs
            .iter()
            .find_map(|(key, value)| (key == variable).then(|| value.clone())))
    }
}

fn into_string(value: OsString, variable: String) -> Result<String, ConfigError> {
    value
        .into_string()
        .map_err(|_| ConfigError::InvalidUnicode { variable })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMON_ENV: [(&str, &str); 3] = [
        (WEBHOOK_KEY, "webhook-secret"),
        (TRMNL_TOKEN, "trmnl-secret"),
        (PUBLIC_BASE_URL, "https://example.test"),
    ];

    const GOOGLE_ENV: [(&str, &str); 3] = [
        (GOOGLE_CALENDAR_CLIENT_ID, "google-client-id"),
        (GOOGLE_CALENDAR_CLIENT_SECRET, "google-client-secret"),
        (GOOGLE_CALENDAR_REFRESH_TOKEN, "google-refresh-token"),
    ];

    const WHATSAPP_ENV: [(&str, &str); 2] = [
        (WHATSAPP_ACCESS_TOKEN, "access-secret"),
        (WHATSAPP_PHONE_NUMBER_ID, "phone-number"),
    ];

    const TELEGRAM_ENV: [(&str, &str); 1] = [(TELEGRAM_BOT_TOKEN, "bot-secret")];

    #[test]
    fn loads_whatsapp_required_values_and_defaults() {
        let config =
            AppConfig::from_pairs(COMMON_ENV.into_iter().chain(GOOGLE_ENV).chain(WHATSAPP_ENV))
                .expect("config should load");

        assert_eq!(config.webhook_key.as_str(), "webhook-secret");
        assert_eq!(config.chat_auth_key, None);
        let MessagingProviderConfig::WhatsApp(whatsapp) = config.messaging_provider else {
            panic!("WhatsApp config should load");
        };
        assert_eq!(whatsapp.access_token.as_str(), "access-secret");
        assert_eq!(whatsapp.phone_number_id, "phone-number");
        assert_eq!(config.google_calendar.client_id, "google-client-id");
        assert_eq!(
            config.google_calendar.client_secret.as_str(),
            "google-client-secret"
        );
        assert_eq!(
            config.google_calendar.refresh_token.as_str(),
            "google-refresh-token"
        );
        assert_eq!(config.trmnl.token.as_str(), "trmnl-secret");
        assert_eq!(config.public_base_url, "https://example.test");
        assert_eq!(config.database_path, PathBuf::from(DEFAULT_DATABASE_PATH));
        assert_eq!(config.bind_addr, DEFAULT_BIND_ADDR);
    }

    #[test]
    fn loads_telegram_required_values() {
        let config =
            AppConfig::from_pairs(COMMON_ENV.into_iter().chain(GOOGLE_ENV).chain(TELEGRAM_ENV))
                .expect("config should load");

        let MessagingProviderConfig::Telegram(telegram) = config.messaging_provider else {
            panic!("Telegram config should load");
        };
        assert_eq!(telegram.bot_token.as_str(), "bot-secret");
    }

    #[test]
    fn optional_values_override_defaults() {
        let config = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain(WHATSAPP_ENV)
                .chain([
                    (DATABASE_PATH, "/tmp/list.db"),
                    (BIND_ADDR, "0.0.0.0:8080"),
                    (CHAT_AUTH_KEY, "chat-secret"),
                ]),
        )
        .expect("config should load");

        assert_eq!(config.database_path, PathBuf::from("/tmp/list.db"));
        assert_eq!(config.bind_addr, "0.0.0.0:8080");
        assert_eq!(
            config.chat_auth_key.as_ref().map(SecretString::as_str),
            Some("chat-secret")
        );
    }

    #[test]
    fn missing_common_required_values_name_the_variable() {
        let error = AppConfig::from_pairs(WHATSAPP_ENV.into_iter().chain(GOOGLE_ENV)).unwrap_err();

        assert_eq!(
            error,
            ConfigError::MissingRequiredVariable {
                variable: WEBHOOK_KEY
            }
        );
        assert!(error.to_string().contains(WEBHOOK_KEY));
    }

    #[test]
    fn missing_google_calendar_value_names_the_variable() {
        let error = AppConfig::from_pairs(COMMON_ENV.into_iter().chain(WHATSAPP_ENV)).unwrap_err();

        assert_eq!(
            error,
            ConfigError::MissingRequiredVariable {
                variable: GOOGLE_CALENDAR_CLIENT_ID
            }
        );
    }

    #[test]
    fn missing_provider_group_fails() {
        let error = AppConfig::from_pairs(COMMON_ENV.into_iter().chain(GOOGLE_ENV)).unwrap_err();

        assert_eq!(error, ConfigError::MissingMessagingProvider);
    }

    #[test]
    fn both_providers_load_when_both_provider_groups_exist() {
        let config = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain(WHATSAPP_ENV)
                .chain(TELEGRAM_ENV),
        )
        .expect("config should load");

        let MessagingProviderConfig::Both { whatsapp, telegram } = config.messaging_provider else {
            panic!("both provider configs should load");
        };
        assert_eq!(whatsapp.access_token.as_str(), "access-secret");
        assert_eq!(whatsapp.phone_number_id, "phone-number");
        assert_eq!(telegram.bot_token.as_str(), "bot-secret");
    }

    #[test]
    fn incomplete_whatsapp_group_is_ignored_when_telegram_can_load() {
        let config = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain([(WHATSAPP_ACCESS_TOKEN, "access-secret")])
                .chain(TELEGRAM_ENV),
        )
        .expect("config should load");

        let MessagingProviderConfig::Telegram(telegram) = config.messaging_provider else {
            panic!("Telegram config should load");
        };
        assert_eq!(telegram.bot_token.as_str(), "bot-secret");
    }

    #[test]
    fn incomplete_whatsapp_provider_group_fails() {
        let error = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain([(WHATSAPP_ACCESS_TOKEN, "access-secret")]),
        )
        .unwrap_err();

        assert_eq!(
            error,
            ConfigError::IncompleteMessagingProvider {
                provider: "WhatsApp"
            }
        );
    }

    #[test]
    fn whatsapp_verify_token_is_not_a_provider_alias() {
        let error = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain([("WHATSAPP_VERIFY_TOKEN", "old-secret")]),
        )
        .unwrap_err();

        assert_eq!(error, ConfigError::MissingMessagingProvider);
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config =
            AppConfig::from_pairs(COMMON_ENV.into_iter().chain(GOOGLE_ENV).chain(WHATSAPP_ENV))
                .expect("config should load");

        assert_eq!(format!("{:?}", config.webhook_key), "[redacted]");
        assert!(!format!("{config:?}").contains("webhook-secret"));
        assert!(!format!("{config:?}").contains("access-secret"));
        assert!(!format!("{config:?}").contains("google-client-secret"));
        assert!(!format!("{config:?}").contains("google-refresh-token"));

        let config = AppConfig::from_pairs(
            COMMON_ENV
                .into_iter()
                .chain(GOOGLE_ENV)
                .chain(WHATSAPP_ENV)
                .chain([(CHAT_AUTH_KEY, "chat-secret")]),
        )
        .expect("config should load");
        assert!(!format!("{config:?}").contains("chat-secret"));
    }

    #[test]
    fn invalid_unicode_errors_do_not_include_values() {
        let result = EnvSource::new([(OsString::from(WEBHOOK_KEY), invalid_unicode_os_string())]);
        let Err(error) = result else {
            panic!("invalid unicode should produce a config error");
        };

        let debug = format!("{error:?}");
        let display = error.to_string();

        assert!(debug.contains(WEBHOOK_KEY));
        assert!(display.contains(WEBHOOK_KEY));
    }

    #[cfg(unix)]
    fn invalid_unicode_os_string() -> OsString {
        use std::os::unix::ffi::OsStringExt;

        OsString::from_vec(vec![0xff])
    }

    #[cfg(windows)]
    fn invalid_unicode_os_string() -> OsString {
        use std::os::windows::ffi::OsStringExt;

        OsString::from_wide(&[0xD800])
    }
}
