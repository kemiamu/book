use std::sync::LazyLock;

pub mod crypto;
pub mod model;
#[cfg(test)]
pub mod tests;

/// global config
pub static CONFIG: LazyLock<config::Config> =
    LazyLock::new(|| config::Config::init("server.toml").expect("failed to load config"));
/// global templates
pub static TEMPLATES: LazyLock<tera::Tera> =
    LazyLock::new(|| tera::Tera::new("templates/**/*").expect("failed to load templates"));

pub mod config {
    #[derive(serde::Deserialize)]
    /// server configuration
    pub struct Config {
        pub server_addr: String,
        pub site_root: String,
        pub base_url: String,
        pub site_title: String,
        pub secret: String,
    }

    impl Config {
        /// load config from toml file
        pub fn init(file: impl AsRef<std::path::Path>) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(toml::from_str(&std::fs::read_to_string(file)?)?)
        }
    }
}
