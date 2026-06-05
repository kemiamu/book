use std::sync::LazyLock;

pub mod crypto;
#[cfg(test)]
pub mod tests;

pub static CONFIG: LazyLock<config::Config> =
    LazyLock::new(|| config::Config::init("server.toml").expect("failed to load config"));
pub static TEMPLATES: LazyLock<tera::Tera> =
    LazyLock::new(|| tera::Tera::new("templates/**/*").expect("failed to load templates"));

pub mod model {
    pub mod error;
    pub mod res;
    pub mod user;

    // state

    pub struct AppState {
        pub db: redb::Database,
    }

    // context

    pub struct PageContext(tera::Context);

    impl PageContext {
        pub fn new() -> Self {
            let mut ctx = tera::Context::new();
            ctx.insert("site_title", &crate::CONFIG.site_title);
            ctx.insert("base_url", &crate::CONFIG.base_url);
            Self(ctx)
        }

        pub fn insert<T: serde::Serialize + ?Sized>(mut self, key: &str, val: &T) -> Self {
            self.0.insert(key, val);
            self
        }

        pub fn render(self, template: &str) -> Result<String, tera::Error> {
            crate::TEMPLATES.render(template, &self.0)
        }
    }
}

pub mod config {
    #[derive(serde::Deserialize)]
    pub struct Config {
        pub server_addr: String,
        pub site_root: String,
        pub base_url: String,
        pub site_title: String,
        pub secret: String,
    }

    impl Config {
        pub fn init(file: impl AsRef<std::path::Path>) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(toml::from_str(&std::fs::read_to_string(file)?)?)
        }
    }
}
