#![forbid(unsafe_code)]

mod config;

fn main() -> Result<(), config::ConfigError> {
    let _config = config::AppConfig::from_env()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn executable_package_metadata_is_declared() {
        assert_eq!(env!("CARGO_PKG_NAME"), "trmnl-whatsapp-list");
    }
}
