#![forbid(unsafe_code)]

mod app;
mod config;
mod http;
mod store;

fn main() -> Result<(), StartupError> {
    let config = config::AppConfig::from_env()?;
    let state = app::AppState::new(config)?;
    let _router = http::router(state);

    Ok(())
}

#[derive(Debug)]
enum StartupError {
    Config(config::ConfigError),
    Store(store::StoreError),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Store(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for StartupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Store(error) => Some(error),
        }
    }
}

impl From<config::ConfigError> for StartupError {
    fn from(error: config::ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<store::StoreError> for StartupError {
    fn from(error: store::StoreError) -> Self {
        Self::Store(error)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn executable_package_metadata_is_declared() {
        assert_eq!(env!("CARGO_PKG_NAME"), "trmnl-whatsapp-list");
    }
}
