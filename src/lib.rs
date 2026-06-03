#[cfg(test)]
pub mod tests;

pub mod model {
    pub mod res;
    pub mod user;
    use crate::config::Config;
    use redb::Database;
    use rkyv::{Archive, Deserialize, Serialize};
    use tera::Tera;

    // state

    pub struct AppState {
        pub config: Config,
        pub db: Database,
        pub templates: Tera,
    }

    // mac

    #[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(transparent)]
    pub struct Mac([u8; 32]);

    impl Mac {
        pub fn new(input: impl AsRef<[u8]>, key: impl AsRef<[u8]>, tag: impl AsRef<[u8]>) -> Self {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.update(input);
            hasher.update(tag);
            Self(hasher.finalize().into())
        }
    }

    impl std::fmt::Display for Mac {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", hex::encode(self.0))
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
        pub fn init(file: &str) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(toml::from_str(&std::fs::read_to_string(file)?)?)
        }
    }
}
