use crate::{crypto::Mac, crypto::Signable, crypto::Signed, error::AppError, html::HtmlWriter};
use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use axum_extra::extract::cookie::CookieJar;
use redb::TableDefinition as Table;
use std::{borrow::Cow, marker::PhantomData, path::Path};

/// entry key: (category, entry)
pub type EntryPath = (Slug<CategoryKey>, Slug<EntryKey>);
/// file key: (category, entry, file)
pub type FilePath = (Slug<CategoryKey>, Slug<EntryKey>, Slug<FileKey>);

/// entries table definition, keyed by (category, entry)
pub const ENTRIES: Table<EntryPath, EntryMeta> = Table::new("entries");
/// entry body table definition (raw markdown + rendered html)
pub const ENTRY_BODY: Table<EntryPath, EntryBody> = Table::new("entry_body");

/// files table definition, keyed by (category, entry, file)
pub const FILES: Table<FilePath, FileMeta> = Table::new("files");
/// file blob table definition
pub const FILE_BLOB: Table<FilePath, Vec<u8>> = Table::new("file_blob");

/// users table definition
pub const USERS: Table<Slug<UserKey>, User> = Table::new("users");

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
        ctx.insert("base_path", &crate::CONFIG.base_path());
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
    pub editor: Slug<UserKey>,
    pub last_modified: i64,
}

impl FileMeta {
    /// create new resource metadata with current timestamp
    pub fn new<E: Into<Slug<UserKey>>>(editor: E) -> Self {
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
    pub editor: Slug<UserKey>,
    pub created_at: i64,
    pub last_modified: i64,
}

impl EntryMeta {
    /// create new entry metadata
    pub fn new<T: Into<String>, E: Into<Slug<UserKey>>>(title: T, editor: E) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            title: title.into(),
            editor: editor.into(),
            created_at: now,
            last_modified: now,
        }
    }

    /// update metadata, preserving the creation time
    pub fn update<T: Into<String>, E: Into<Slug<UserKey>>>(&self, title: T, editor: E) -> Self {
        Self {
            title: title.into(),
            editor: editor.into(),
            created_at: self.created_at,
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
    pub parent: Option<Slug<UserKey>>,
}

impl User {
    const PASSWD_TAG: &str = "password";

    /// create a new user
    pub fn new<P: AsRef<[u8]>, S: AsRef<[u8]>>(
        password: P,
        secret: S,
        parent: Option<Slug<UserKey>>,
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
    creator: Option<Slug<UserKey>>,
    expires_at: i64,
}

impl Passkey {
    pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;
    const BOOTSTRAP_EXPIRY_SECS: i64 = 24 * 60 * 60;

    /// create an invitation passkey for a known user
    pub fn new<C: Into<Slug<UserKey>>>(creator: C) -> Self {
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
    pub fn creator(&self) -> Option<&Slug<UserKey>> {
        self.creator.as_ref()
    }

    /// consume the passkey, returning its creator
    pub fn into_creator(self) -> Option<Slug<UserKey>> {
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
    pub user: Slug<UserKey>,
    pub expires_at: i64,
}

impl Session {
    pub const EXPIRY_SECS: i64 = 3650 * 24 * 60 * 60;

    /// create a new session
    pub fn new<U: Into<Slug<UserKey>>>(user: U) -> Self {
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
pub struct UserToken(pub Result<Slug<UserKey>, AppError>);

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

/// validation rules for a kind of slug: max length + allowed characters
pub trait SlugRule {
    /// maximum length in bytes
    const MAX_LEN: usize;

    /// whether a character is allowed
    fn is_allowed(ch: char) -> bool;
}

/// define a slug marker type with its validation rule
macro_rules! slug_key {
    ($name:ident, $len:expr, $($ch:literal)|+) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name;

        impl SlugRule for $name {
            const MAX_LEN: usize = $len;

            fn is_allowed(ch: char) -> bool {
                ch.is_ascii_alphanumeric() || matches!(ch, $($ch)|+)
            }
        }
    };
}

slug_key!(CategoryKey, 255, '-' | '_');
slug_key!(EntryKey, 255, '-' | '_');
slug_key!(FileKey, 255, '-' | '_' | '.');
slug_key!(UserKey, 32, '-' | '_');

/// validated slug: non-empty, single URL path segment, tagged with its rule
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(bound(serialize = ""), transparent)]
#[repr(transparent)]
pub struct Slug<T: SlugRule>(Cow<'static, str>, PhantomData<T>);

impl<T: SlugRule> Slug<T> {
    /// validate and create a new slug
    pub fn new<V: Into<Cow<'static, str>>>(value: V) -> Result<Self, &'static str> {
        let cow = value.into();
        if cow.is_empty() {
            Err("slug must not be empty")
        } else if cow.len() > T::MAX_LEN {
            Err("slug exceeds the max length")
        } else if !cow.chars().all(T::is_allowed) {
            Err("slug contains disallowed characters")
        } else {
            Ok(Self(cow, PhantomData))
        }
    }

    /// split a string on disallowed characters into valid slugs
    pub fn split(input: &str) -> impl Iterator<Item = Slug<T>> {
        input
            .split(|ch: char| !T::is_allowed(ch))
            .filter_map(|seg| Slug::new(seg.to_string()).ok())
    }
}

impl<T: SlugRule> std::fmt::Display for Slug<T> {
    /// format as plain string
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: SlugRule> AsRef<str> for Slug<T> {
    /// borrow the underlying string
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<T: SlugRule> std::ops::Deref for Slug<T> {
    type Target = str;

    /// deref to the underlying string
    fn deref(&self) -> &str {
        &self.0
    }
}

impl<'de, T: SlugRule> serde::Deserialize<'de> for Slug<T> {
    /// deserialize with validation
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Slug::new(raw).map_err(serde::de::Error::custom)
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

/// implement redb::Key by raw byte comparison
macro_rules! impl_key {
    ($ty:ty) => {
        impl redb::Key for $ty {
            /// keys are ordered by raw bytes, same as `str`
            fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
                a.cmp(b)
            }
        }
    };
}

impl_stored!(FileMeta);
impl_stored!(EntryMeta);
impl_stored!(Markdown);
impl_stored!(EntryBody);
impl_stored!(User);
impl_stored!(Slug<CategoryKey>);
impl_stored!(Slug<EntryKey>);
impl_stored!(Slug<FileKey>);
impl_stored!(Slug<UserKey>);

impl_key!(Slug<CategoryKey>);
impl_key!(Slug<EntryKey>);
impl_key!(Slug<FileKey>);
impl_key!(Slug<UserKey>);
