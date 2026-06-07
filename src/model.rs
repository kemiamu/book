use crate::crypto::{Mac, Signable, Signed};
use crate::error::AppError;
use crate::html::HtmlWriter;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use redb::TableDefinition as Table;
use std::collections::HashSet;

/// entries table definition
pub const ENTRIES: Table<&str, EntryMeta> = Table::new("entries");
/// entry raw markdown table definition
pub const ENTRY_RAW: Table<&str, Markdown> = Table::new("entry_raw");
/// entry rendered html table definition
pub const ENTRY_HTML: Table<&str, String> = Table::new("entry_html");

/// files table definition
pub const FILES: Table<(&str, &str), FileMeta> = Table::new("files");
/// file blob table definition
pub const FILE_BLOB: Table<(&str, &str), Vec<u8>> = Table::new("file_blob");

/// users table definition
pub const USERS: Table<&str, User> = Table::new("users");

// state

/// application state
pub struct AppState {
    pub db: redb::Database,
}

// context

/// page render context
pub struct PageContext(tera::Context);

impl PageContext {
    /// create a new context
    pub fn new() -> Self {
        let mut ctx = tera::Context::new();
        ctx.insert("site_title", &crate::CONFIG.site_title);
        ctx.insert("base_url", &crate::CONFIG.base_url);
        Self(ctx)
    }

    /// insert a template variable
    pub fn insert<T: serde::Serialize + ?Sized>(mut self, key: &str, val: &T) -> Self {
        self.0.insert(key, val);
        self
    }

    /// render the template to string
    pub fn render(self, template: &str) -> Result<String, tera::Error> {
        crate::TEMPLATES.render(template, &self.0)
    }
}

// resource types

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// file metadata
pub struct FileMeta {
    pub editor: String,
    pub last_modified: i64,
}

impl FileMeta {
    /// create new resource metadata with current timestamp
    pub fn new(editor: impl Into<String>) -> Self {
        Self {
            editor: editor.into(),
            last_modified: time::UtcDateTime::now().unix_timestamp(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// metadata for entries
pub struct EntryMeta {
    pub title: String,
    pub tags: HashSet<String>,
    pub editor: String,
    pub last_modified: i64,
}

impl EntryMeta {
    /// create new entry metadata
    pub fn new(title: impl Into<String>, editor: impl Into<String>, tags: HashSet<String>) -> Self {
        Self {
            title: title.into(),
            tags,
            editor: editor.into(),
            last_modified: time::UtcDateTime::now().unix_timestamp(),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[repr(transparent)]
/// markdown content wrapper
pub struct Markdown(String);

impl Markdown {
    /// create markdown from string
    pub fn new(content: impl Into<String>) -> Self {
        Self(content.into())
    }

    /// get the raw markdown text
    pub fn into_inner(self) -> String {
        self.0
    }

    /// render markdown to html
    pub fn render(&self) -> String {
        use pulldown_cmark as markdown;
        let parser = markdown::Parser::new_ext(&self.0, markdown::Options::all());
        let mut html_output: String = Default::default();
        HtmlWriter::new(parser, &mut html_output).run().unwrap();
        html_output
    }
}

// user types

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// a registered user
pub struct User {
    password: Mac,
    pub parent: String,
}

impl User {
    const PASSWD_TAG: &str = "password";

    /// create a new user
    pub fn new(
        password: impl AsRef<[u8]>,
        secret: impl AsRef<[u8]>,
        parent: impl Into<String>,
    ) -> Self {
        let password = Mac::new(password, secret, Self::PASSWD_TAG);
        let parent = parent.into();
        Self { password, parent }
    }

    /// verify password against stored hash
    pub fn verify(&self, password: impl AsRef<[u8]>, secret: impl AsRef<[u8]>) -> bool {
        let expected = Mac::new(password, secret, Self::PASSWD_TAG);
        self.password == expected
    }
}

// invite

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// sign-up invitation token
pub struct Invitation {
    pub inviter: String,
    pub expires_at: i64,
}

impl Invitation {
    pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;

    /// create a new invitation
    pub fn new(inviter: impl Into<String>) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            inviter: inviter.into(),
            expires_at: now + Self::EXPIRY_SECS,
        }
    }
}

impl Signable for Invitation {
    /// invitation type tag
    fn tag() -> &'static str {
        "invitation"
    }
    /// check if invitation is not expired
    fn is_valid(&self) -> bool {
        self.expires_at >= time::UtcDateTime::now().unix_timestamp()
    }
    /// serialize invitation to bytes
    fn serialize(&self) -> Vec<u8> {
        postcard::to_stdvec(self).unwrap()
    }
    /// deserialize invitation from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

// session

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// user session token
pub struct Session {
    pub user: String,
    pub expires_at: i64,
}

impl Session {
    pub const EXPIRY_SECS: i64 = 90 * 24 * 60 * 60;

    /// create a new session
    pub fn new(user: impl Into<String>) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            user: user.into(),
            expires_at: now + Self::EXPIRY_SECS,
        }
    }
}

impl Signable for Session {
    /// session type tag
    fn tag() -> &'static str {
        "session"
    }
    /// check if session is not expired
    fn is_valid(&self) -> bool {
        self.expires_at >= time::UtcDateTime::now().unix_timestamp()
    }
    /// serialize session to bytes
    fn serialize(&self) -> Vec<u8> {
        postcard::to_stdvec(self).unwrap()
    }
    /// deserialize session from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

// token

/// authenticated user extracted from session cookie
#[derive(Debug)]
pub struct UserToken(pub Result<String, AppError>);

impl<S: Send + Sync + 'static> FromRequestParts<S> for UserToken {
    type Rejection = std::convert::Infallible;

    /// extract user from session cookie
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_request_parts(parts, &())
            .await
            .unwrap_or_default();

        let Some(cookie) = jar.get("session") else {
            return Ok(UserToken(Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Not signed in",
            ))));
        };

        let Some(session) = Signed::<Session>::parse(cookie.value(), &crate::CONFIG.secret) else {
            return Ok(UserToken(Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Invalid or expired session",
            ))));
        };

        Ok(UserToken(Ok(session.inner.user)))
    }
}

// store

/// implement redb::Value via postcard for a serde type
#[macro_export]
macro_rules! impl_stored {
    ($ty:ty) => {
        impl redb::Value for $ty {
            type SelfType<'a>
                = $ty
            where
                Self: 'a;
            type AsBytes<'a>
                = Vec<u8>
            where
                Self: 'a;

            fn type_name() -> redb::TypeName {
                redb::TypeName::new(stringify!($ty))
            }

            fn fixed_width() -> Option<usize> {
                None
            }

            fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
            where
                Self: 'a,
            {
                postcard::from_bytes(data).unwrap()
            }

            fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
            where
                Self: 'b,
            {
                postcard::to_stdvec(value).unwrap()
            }
        }
    };
}

impl_stored!(FileMeta);
impl_stored!(EntryMeta);
impl_stored!(Markdown);
impl_stored!(User);
