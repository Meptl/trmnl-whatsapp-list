use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

const WHATSAPP_VERIFY_TOKEN: &str = "WHATSAPP_VERIFY_TOKEN";
const WHATSAPP_ACCESS_TOKEN: &str = "WHATSAPP_ACCESS_TOKEN";
const WHATSAPP_PHONE_NUMBER_ID: &str = "WHATSAPP_PHONE_NUMBER_ID";
const TRMNL_TOKEN: &str = "TRMNL_TOKEN";
const PUBLIC_BASE_URL: &str = "PUBLIC_BASE_URL";
const DATABASE_PATH: &str = "DATABASE_PATH";
const BIND_ADDR: &str = "BIND_ADDR";

const DEFAULT_DATABASE_PATH: &str = "list.db";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";

#[allow(dead_code)]
pub struct AppConfig {
    pub whatsapp: WhatsAppConfig,
    pub trmnl: TrmnlConfig,
    pub public_base_url: String,
    pub database_path: PathBuf,
    pub bind_addr: String,
}

#[allow(dead_code)]
pub struct WhatsAppConfig {
    pub verify_token: SecretString,
    pub access_token: SecretString,
    pub phone_number_id: String,
}

#[allow(dead_code)]
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
            .field("whatsapp", &self.whatsapp)
            .field("trmnl", &self.trmnl)
            .field("public_base_url", &self.public_base_url)
            .field("database_path", &self.database_path)
            .field("bind_addr", &self.bind_addr)
            .finish()
    }
}

impl fmt::Debug for WhatsAppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WhatsAppConfig")
            .field("verify_token", &self.verify_token)
            .field("access_token", &self.access_token)
            .field("phone_number_id", &self.phone_number_id)
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
            whatsapp: WhatsAppConfig {
                verify_token: SecretString(source.required(WHATSAPP_VERIFY_TOKEN)?),
                access_token: SecretString(source.required(WHATSAPP_ACCESS_TOKEN)?),
                phone_number_id: source.required(WHATSAPP_PHONE_NUMBER_ID)?,
            },
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

    const REQUIRED_ENV: [(&str, &str); 5] = [
        (WHATSAPP_VERIFY_TOKEN, "verify-secret"),
        (WHATSAPP_ACCESS_TOKEN, "access-secret"),
        (WHATSAPP_PHONE_NUMBER_ID, "phone-number"),
        (TRMNL_TOKEN, "trmnl-secret"),
        (PUBLIC_BASE_URL, "https://example.test"),
    ];

    #[test]
    fn loads_required_values_and_defaults() {
        let config = AppConfig::from_pairs(REQUIRED_ENV).expect("config should load");

        assert_eq!(config.whatsapp.verify_token.as_str(), "verify-secret");
        assert_eq!(config.whatsapp.access_token.as_str(), "access-secret");
        assert_eq!(config.whatsapp.phone_number_id, "phone-number");
        assert_eq!(config.trmnl.token.as_str(), "trmnl-secret");
        assert_eq!(config.public_base_url, "https://example.test");
        assert_eq!(config.database_path, PathBuf::from(DEFAULT_DATABASE_PATH));
        assert_eq!(config.bind_addr, DEFAULT_BIND_ADDR);
    }

    #[test]
    fn optional_values_override_defaults() {
        let config = AppConfig::from_pairs(
            REQUIRED_ENV
                .into_iter()
                .chain([(DATABASE_PATH, "/tmp/list.db"), (BIND_ADDR, "0.0.0.0:8080")]),
        )
        .expect("config should load");

        assert_eq!(config.database_path, PathBuf::from("/tmp/list.db"));
        assert_eq!(config.bind_addr, "0.0.0.0:8080");
    }

    #[test]
    fn missing_required_values_name_the_variable() {
        let error = AppConfig::from_pairs([(WHATSAPP_ACCESS_TOKEN, "access-secret")]).unwrap_err();

        assert_eq!(
            error,
            ConfigError::MissingRequiredVariable {
                variable: WHATSAPP_VERIFY_TOKEN
            }
        );
        assert!(error.to_string().contains(WHATSAPP_VERIFY_TOKEN));
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config = AppConfig::from_pairs(REQUIRED_ENV).expect("config should load");

        assert_eq!(format!("{:?}", config.whatsapp.verify_token), "[redacted]");
        assert!(!format!("{:?}", config.whatsapp.verify_token).contains("verify-secret"));
    }

    #[test]
    fn invalid_unicode_errors_do_not_include_values() {
        let result = EnvSource::new([(
            OsString::from(WHATSAPP_VERIFY_TOKEN),
            invalid_unicode_os_string(),
        )]);
        let Err(error) = result else {
            panic!("invalid unicode should produce a config error");
        };

        let debug = format!("{error:?}");
        let display = error.to_string();

        assert!(debug.contains(WHATSAPP_VERIFY_TOKEN));
        assert!(display.contains(WHATSAPP_VERIFY_TOKEN));
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
