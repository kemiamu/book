use std::sync::LazyLock;

#[cfg(test)]
pub mod tests;

pub static CONFIG: LazyLock<config::Config> =
    LazyLock::new(|| config::Config::init("server.toml").expect("failed to load config"));
pub static TEMPLATES: LazyLock<tera::Tera> =
    LazyLock::new(|| tera::Tera::new("templates/**/*").expect("failed to load templates"));

pub mod model {
    use crate::{CONFIG, TEMPLATES};
    use axum::http::StatusCode;
    use axum::response::{Html, IntoResponse, Response};
    use redb::Database;
    use rkyv::{Archive, Deserialize, Serialize};

    pub mod res;
    pub mod user;

    // state

    pub struct AppState {
        pub db: Database,
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

    // error

    type BoxErr = Box<dyn std::error::Error + Send + Sync>;

    pub struct AppError {
        inner: BoxErr,
        status: StatusCode,
    }

    impl AppError {
        pub fn new(status: StatusCode, msg: impl Into<BoxErr>) -> Self {
            let inner = msg.into();
            Self { inner, status }
        }
    }

    impl IntoResponse for AppError {
        fn into_response(self) -> Response {
            tracing::error!("{}", self.inner);

            let mut ctx = tera::Context::new();
            ctx.insert("site_title", &CONFIG.site_title);
            ctx.insert("code", &self.status.as_u16());
            ctx.insert("reason", &self.status.canonical_reason().unwrap_or("Error"));
            ctx.insert("message", &self.inner.to_string());

            let html = TEMPLATES.render("error.html", &ctx).unwrap();
            (self.status, Html(html)).into_response()
        }
    }

    macro_rules! impl_from {
        ($ty:ty, $status:expr) => {
            impl From<$ty> for AppError {
                fn from(e: $ty) -> Self {
                    Self::new($status, e)
                }
            }
        };
    }

    impl_from!(std::io::Error, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(redb::Error, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(redb::StorageError, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(redb::TableError, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(redb::TransactionError, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(redb::CommitError, StatusCode::INTERNAL_SERVER_ERROR);
    impl_from!(tera::Error, StatusCode::INTERNAL_SERVER_ERROR);
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
