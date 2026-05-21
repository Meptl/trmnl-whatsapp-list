#![forbid(unsafe_code)]

mod app;
mod config;
mod http;

fn main() -> Result<(), config::ConfigError> {
    let config = config::AppConfig::from_env()?;
    let state = app::AppState::new(config);
    let _router = http::router(state);

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn executable_package_metadata_is_declared() {
        assert_eq!(env!("CARGO_PKG_NAME"), "trmnl-whatsapp-list");
    }
}
