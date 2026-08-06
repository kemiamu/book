use std::sync::LazyLock;

pub mod html;
pub mod model;

/// global config
pub static CONFIG: LazyLock<config::Config> =
    LazyLock::new(|| config::Config::init("server.toml").expect("failed to load config"));
/// global templates
pub static TEMPLATES: LazyLock<tera::Tera> =
    LazyLock::new(|| tera::Tera::new("templates/**/*").expect("failed to load templates"));

pub mod crypto {
    // mac
    //
    // ++++++++++++============++++++++++++============++++++++++++============

    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[repr(transparent)]
    /// message authentication code
    pub struct Mac([u8; 32]);

    impl Mac {
        /// create a mac from input with secret and tag
        pub fn new<I: AsRef<[u8]>, S: AsRef<[u8]>, T: AsRef<[u8]>>(
            input: I,
            secret: S,
            tag: T,
        ) -> Self {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(input);
            hasher.update(secret);
            hasher.update(tag);
            Self(hasher.finalize().into())
        }
    }

    impl std::fmt::Display for Mac {
        /// format as hex string
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", hex::encode(self.0))
        }
    }

    // signable / signed
    //
    // ++++++++++++============++++++++++++============++++++++++++============

    /// signable token trait
    pub trait Signable: Sized {
        /// type tag string
        fn tag() -> &'static str;
        /// serialize to bytes
        fn serialize(&self) -> Vec<u8>;
        /// deserialize from bytes
        fn deserialize(bytes: &[u8]) -> Option<Self>;
        /// check if the value is still valid
        fn is_valid(&self) -> bool;
    }

    /// signed wrapper with mac
    pub struct Signed<T: Signable> {
        pub inner: T,
    }

    impl<T: Signable> Signed<T> {
        /// wrap a value
        pub fn new(inner: T) -> Self {
            Self { inner }
        }

        /// parse a signed string
        pub fn parse<E: AsRef<str>, S: AsRef<[u8]>>(encoded: E, secret: S) -> Option<Self> {
            let (hex, sig) = encoded.as_ref().rsplit_once('.')?;
            let data = hex::decode(hex).ok()?;
            let inner = T::deserialize(&data)?;
            let expected = Mac::new(&data, secret, T::tag()).to_string();
            (sig == expected && inner.is_valid()).then_some(Self { inner })
        }

        /// generate a signed string
        pub fn generate<S: AsRef<[u8]>>(&self, secret: S) -> String {
            let data = self.inner.serialize();
            let data_hex = hex::encode(&data);
            let sig = Mac::new(&data, secret, T::tag());
            format!("{data_hex}.{sig}")
        }
    }
}

pub mod config {
    use std::{error::Error, path::Path};

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
        pub fn init<P: AsRef<Path>>(file: P) -> Result<Self, Box<dyn Error>> {
            Ok(toml::from_str(&std::fs::read_to_string(file)?)?)
        }

        /// the path prefix of base_url, e.g. "/book" for "https://kemya.net/book"
        pub fn base_path(&self) -> &str {
            let rest = self
                .base_url
                .split_once("://")
                .map(|(_, rest)| rest)
                .unwrap_or(&self.base_url);
            rest.find('/').map(|i| &rest[i..]).unwrap_or("")
        }
    }
}

pub mod error {
    // app error
    //
    // ++++++++++++============++++++++++++============++++++++++++============

    use crate::model::{PageContext, Slug, UserName};
    use axum::{
        Json, http::StatusCode, response::Html, response::IntoResponse, response::Response,
    };

    type BoxErr = Box<dyn std::error::Error + Send + Sync>;

    /// application error with status and message
    pub struct AppError {
        status: StatusCode,
        inner: BoxErr,
        json: bool,
    }

    impl std::fmt::Debug for AppError {
        /// debug format for logging
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AppError")
                .field("status", &self.status)
                .field("inner", &self.inner)
                .finish()
        }
    }

    impl AppError {
        /// create a new app error
        pub fn new<M: Into<BoxErr>>(status: StatusCode, msg: M) -> Self {
            Self {
                inner: msg.into(),
                status,
                json: false,
            }
        }

        /// create an error that responds with json instead of an html page
        pub fn json<M: Into<BoxErr>>(status: StatusCode, msg: M) -> Self {
            Self {
                inner: msg.into(),
                status,
                json: true,
            }
        }
    }

    impl IntoResponse for AppError {
        /// render error as html page, or json for api endpoints
        fn into_response(self) -> Response {
            tracing::error!("{:?}", self);

            if self.json {
                return (
                    self.status,
                    Json(serde_json::json!({"error": self.inner.to_string()})),
                )
                    .into_response();
            }

            let html = PageContext::new()
                .insert("code", &self.status.as_u16())
                .insert("reason", &self.status.canonical_reason().unwrap_or("Error"))
                .insert("message", &self.inner.to_string())
                .insert("user", &None::<Slug<UserName>>)
                .render("error.html")
                .unwrap();
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
