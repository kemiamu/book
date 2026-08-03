use crate::crypto::{Mac, Signable, Signed};
use crate::error::AppError;
use crate::html::HtmlWriter;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use redb::TableDefinition as Table;
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;

/// entries table definition
pub const ENTRIES: Table<Slug, EntryMeta> = Table::new("entries");
/// entry body table definition (raw markdown + rendered html)
pub const ENTRY_BODY: Table<Slug, EntryBody> = Table::new("entry_body");

/// files table definition
pub const FILES: Table<(Slug, Slug), FileMeta> = Table::new("files");
/// file blob table definition
pub const FILE_BLOB: Table<(Slug, Slug), Vec<u8>> = Table::new("file_blob");

/// users table definition
pub const USERS: Table<Username, User> = Table::new("users");

/// application state
pub struct AppState {
    pub db: redb::Database,
}

/// create the database and all tables, printing a bootstrap passkey
pub fn init_tables<P: AsRef<Path>>(path: P) -> Result<redb::Database, redb::Error> {
    let db = redb::Database::create(path)?;
    let tx = db.begin_write()?;
    tx.open_table(ENTRIES)?;
    tx.open_table(ENTRY_BODY)?;
    tx.open_table(FILES)?;
    tx.open_table(FILE_BLOB)?;
    tx.open_table(USERS)?;
    tx.commit()?;

    let base_url = &crate::CONFIG.base_url;
    let passkey = Signed::new(Passkey::bootstrap()).generate(&crate::CONFIG.secret);
    tracing::info!("Bootstrap passkey: {base_url}/auth?passkey={passkey}");

    Ok(db)
}

// context
//
// ++++++++++++============++++++++++++============++++++++++++============

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
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// file metadata
pub struct FileMeta {
    pub editor: Username,
    pub last_modified: i64,
}

impl FileMeta {
    /// create new resource metadata with current timestamp
    pub fn new<E: Into<Username>>(editor: E) -> Self {
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
    pub tags: HashSet<Slug>,
    pub editor: Username,
    pub last_modified: i64,
}

impl EntryMeta {
    /// create new entry metadata
    pub fn new<T: Into<String>, E: Into<Username>, I: IntoIterator<Item = Slug>>(
        title: T,
        editor: E,
        tags: I,
    ) -> Self {
        Self {
            title: title.into(),
            tags: tags.into_iter().collect(),
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
    pub fn new<C: Into<String>>(content: C) -> Self {
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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// entry content: raw markdown plus its rendered html
pub struct EntryBody {
    pub raw: Markdown,
    pub html: String,
}

impl EntryBody {
    /// render a new body from markdown
    pub fn new(raw: Markdown) -> Self {
        let html = raw.render();
        Self { raw, html }
    }
}

// user / passkey
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// a registered user
pub struct User {
    password: Mac,
    pub parent: Option<Username>,
}

impl User {
    const PASSWD_TAG: &str = "password";

    /// create a new user
    pub fn new<P: AsRef<[u8]>, S: AsRef<[u8]>>(
        password: P,
        secret: S,
        parent: Option<Username>,
    ) -> Self {
        let password = Mac::new(password, secret, Self::PASSWD_TAG);
        Self { password, parent }
    }

    /// verify password against stored hash
    pub fn verify<P: AsRef<[u8]>, S: AsRef<[u8]>>(&self, password: P, secret: S) -> bool {
        let expected = Mac::new(password, secret, Self::PASSWD_TAG);
        self.password == expected
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// authorization passkey token
pub struct Passkey {
    creator: Option<Username>,
    expires_at: i64,
}

impl Passkey {
    pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;
    const BOOTSTRAP_EXPIRY_SECS: i64 = 24 * 60 * 60;

    /// create an invitation passkey for a known user
    pub fn new<C: Into<Username>>(creator: C) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            creator: Some(creator.into()),
            expires_at: now + Self::EXPIRY_SECS,
        }
    }

    /// create a bootstrap passkey for first-time initialization
    pub fn bootstrap() -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            creator: None,
            expires_at: now + Self::BOOTSTRAP_EXPIRY_SECS,
        }
    }

    /// the inviting user, if this is an invitation passkey
    pub fn creator(&self) -> Option<&Username> {
        self.creator.as_ref()
    }

    /// consume the passkey, returning its creator
    pub fn into_creator(self) -> Option<Username> {
        self.creator
    }

    /// expiry timestamp
    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

impl Signable for Passkey {
    /// passkey type tag
    fn tag() -> &'static str {
        "passkey"
    }
    /// check if passkey is not expired
    fn is_valid(&self) -> bool {
        self.expires_at >= time::UtcDateTime::now().unix_timestamp()
    }
    /// serialize passkey to bytes
    fn serialize(&self) -> Vec<u8> {
        postcard::to_stdvec(self).unwrap()
    }
    /// deserialize passkey from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

// session / token
//
// ++++++++++++============++++++++++++============++++++++++++============

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
/// user session token
pub struct Session {
    pub user: Username,
    pub expires_at: i64,
}

impl Session {
    pub const EXPIRY_SECS: i64 = 3650 * 24 * 60 * 60;

    /// create a new session
    pub fn new<U: Into<Username>>(user: U) -> Self {
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

/// authenticated user extracted from session cookie
#[derive(Debug)]
pub struct UserToken(pub Result<Username, AppError>);

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

// slug
//
// ++++++++++++============++++++++++++============++++++++++++============

/// username alias of slug: a slug with the meaning of a user identifier
pub type Username = Slug;

/// validated slug: non-empty, single URL path segment
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Slug(Cow<'static, str>);

impl Slug {
    /// check if a char is allowed in a slug
    fn is_slug_char(ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
    }

    /// validate and create a new slug
    pub fn new<V: Into<Cow<'static, str>>>(value: V) -> Result<Self, &'static str> {
        let cow = value.into();
        if cow.is_empty() {
            Err("slug must not be empty")
        } else if cow.len() > 255 {
            Err("slug must be at most 255 bytes")
        } else if !cow.chars().all(Self::is_slug_char) {
            Err("slug may only contain a-z, A-Z, 0-9, '-', '_' and '.'")
        } else {
            Ok(Self(cow))
        }
    }

    /// split a string on non-slug characters into valid slugs
    pub fn split(input: &str) -> impl Iterator<Item = Slug> {
        input
            .split(|ch: char| !Self::is_slug_char(ch))
            .filter_map(|seg| Slug::new(seg.to_string()).ok())
    }
}

impl std::fmt::Display for Slug {
    /// format as plain string
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for Slug {
    /// borrow the underlying string
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for Slug {
    type Target = str;

    /// deref to the underlying string
    fn deref(&self) -> &str {
        &self.0
    }
}

impl serde::Serialize for Slug {
    /// serialize as a plain string
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Slug {
    /// deserialize with validation
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Slug::new(raw).map_err(serde::de::Error::custom)
    }
}

impl redb::Key for Slug {
    /// keys are ordered by raw bytes, same as `str`
    fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        a.cmp(b)
    }
}

// store
//
// ++++++++++++============++++++++++++============++++++++++++============

/// implement redb::Value via postcard for a serde type
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
impl_stored!(EntryBody);
impl_stored!(User);
impl_stored!(Slug);
