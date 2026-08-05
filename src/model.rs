//! canonical definitions of the application's fundamental data formats

use crate::{crypto::Mac, crypto::Signable, crypto::Signed, error::AppError, html::HtmlWriter};
use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use axum_extra::extract::cookie::CookieJar;
use redb::{ReadableTable, TableDefinition as Table};
use std::{borrow::Cow, marker::PhantomData, path::Path};

/// entries table definition, keyed by hex entry id
pub const ENTRIES: Table<EntryId, EntryMeta> = Table::new("entries");
/// entry body table definition, markdown with its rendered html cache
pub const ENTRY_BODY: Table<EntryId, Markdown<'static>> = Table::new("entry_body");
/// singleton counter that allocates entry ids
pub const ENTRY_COUNTER: Table<(), u64> = Table::new("entry_counter");

/// files table definition, keyed by (entry id, file slug)
pub const FILES: Table<(EntryId, Slug<FileKey>), FileMeta> = Table::new("files");
/// file blob table definition, borrowed slices for zero-copy reads
pub const FILE_BLOB: Table<(EntryId, Slug<FileKey>), &[u8]> = Table::new("file_blob");

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
    tx.open_table(ENTRY_COUNTER)?;
    tx.commit()?;

    let base_url = &crate::CONFIG.base_url;
    let passkey = Signed::new(Passkey::bootstrap()).generate(&crate::CONFIG.secret);
    tracing::info!("Bootstrap passkey: {base_url}/auth?passkey={passkey}");

    Ok(db)
}

/// allocate the next entry id from the singleton counter
pub fn next_entry_id(tx: &mut redb::WriteTransaction) -> Result<EntryId, redb::Error> {
    let mut table = tx.open_table(ENTRY_COUNTER)?;
    let next = table.get(())?.map_or(1, |n| n.value() + 1);
    table.insert(&(), next)?;
    Ok(EntryId::from(next))
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
    pub category: String,
    pub editor: Slug<UserKey>,
    pub created_at: i64,
    pub last_modified: i64,
}

impl EntryMeta {
    /// create new entry metadata
    pub fn new<T: Into<String>, C: Into<Slug<CategoryKey>>, E: Into<Slug<UserKey>>>(
        title: T,
        category: C,
        editor: E,
    ) -> Self {
        let now = time::UtcDateTime::now().unix_timestamp();
        Self {
            title: title.into(),
            category: category.into().to_string(),
            editor: editor.into(),
            created_at: now,
            last_modified: now,
        }
    }

    /// update metadata, preserving the creation time
    pub fn update<T: Into<String>, C: Into<Slug<CategoryKey>>, E: Into<Slug<UserKey>>>(
        &self,
        title: T,
        category: C,
        editor: E,
    ) -> Self {
        Self {
            title: title.into(),
            category: category.into().to_string(),
            editor: editor.into(),
            created_at: self.created_at,
            last_modified: time::UtcDateTime::now().unix_timestamp(),
        }
    }
}

#[derive(serde::Serialize, Debug, Clone)]
/// markdown content with its rendered html cache
pub struct Markdown<'a> {
    raw: Cow<'a, str>,
    html: Cow<'a, str>,
}

impl<'a> Markdown<'a> {
    /// create markdown from string, rendering the html cache immediately
    pub fn new<C: Into<Cow<'a, str>>>(content: C) -> Self {
        let raw = content.into();
        let html = Cow::Owned(Self::render(&raw));
        Self { raw, html }
    }

    /// borrow the raw markdown text
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// borrow the rendered html
    pub fn html(&self) -> &str {
        &self.html
    }

    /// render markdown to html
    fn render(raw: &str) -> String {
        use pulldown_cmark::{Options, Parser};
        let parser = Parser::new_ext(raw, Options::all());
        let mut html_output: String = Default::default();
        HtmlWriter::new(parser, &mut html_output).run().unwrap();
        html_output
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

// entry id
//
// ++++++++++++============++++++++++++============++++++++++++============

/// hex-encoded entry id, allocated from the singleton entry counter
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct EntryId(String);

impl EntryId {
    /// construct from raw bytes without validation (key decoder)
    fn from_raw(data: &[u8]) -> Self {
        Self(String::from_utf8_lossy(data).into_owned())
    }
}

impl From<u64> for EntryId {
    /// format a counter value as lowercase hex
    fn from(counter: u64) -> Self {
        Self(format!("{counter:x}"))
    }
}

impl std::str::FromStr for EntryId {
    type Err = &'static str;

    /// parse a hex id from a path segment, normalized to lowercase
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("entry id must be 1-16 hex digits");
        }
        Ok(Self(s.to_ascii_lowercase()))
    }
}

impl std::fmt::Display for EntryId {
    /// format as plain hex string
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl AsRef<str> for EntryId {
    /// borrow the underlying hex string
    fn as_ref(&self) -> &str {
        &self.0
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

    /// construct from raw bytes without validation (key decoder)
    fn from_raw(data: &[u8]) -> Self {
        Self(
            Cow::Owned(String::from_utf8_lossy(data).into_owned()),
            PhantomData,
        )
    }

    /// split a string on disallowed characters into valid slugs
    pub fn split(input: &str) -> impl Iterator<Item = Slug<T>> {
        input
            .split(|ch: char| !T::is_allowed(ch))
            .filter_map(|seg| Slug::new(seg.to_string()).ok())
    }

    /// convert a raw name into a valid slug, replacing runs of
    /// disallowed characters with '-'
    pub fn normalize(input: &str) -> Option<Slug<T>> {
        let mut parts = Self::split(input);
        let mut out = parts.next()?.to_string();
        for part in parts {
            out.push('-');
            out.push_str(&part);
        }
        Slug::new(out).ok()
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

/// implement redb::Value + Key with raw utf8 bytes, so byte order matches
/// string order (the same encoding redb uses for `str` keys)
macro_rules! impl_key {
    ($ty:ty) => {
        impl redb::Value for $ty {
            type SelfType<'a>
                = $ty
            where
                Self: 'a;
            type AsBytes<'a>
                = &'a [u8]
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
                <$ty>::from_raw(data)
            }

            fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
            where
                Self: 'b,
            {
                value.as_ref().as_bytes()
            }
        }

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
impl_stored!(User);

/// value layout: u32 LE raw length, raw utf8, html utf8 (the rest)
impl redb::Value for Markdown<'static> {
    type SelfType<'a>
        = Markdown<'a>
    where
        Self: 'a;
    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn type_name() -> redb::TypeName {
        redb::TypeName::new("Markdown")
    }

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        let raw_len = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        let raw = std::str::from_utf8(&data[4..4 + raw_len]).unwrap();
        let html = std::str::from_utf8(&data[4 + raw_len..]).unwrap();
        Markdown {
            raw: Cow::Borrowed(raw),
            html: Cow::Borrowed(html),
        }
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        let raw = value.raw.as_bytes();
        let html = value.html.as_bytes();
        let mut out = Vec::with_capacity(4 + raw.len() + html.len());
        out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        out.extend_from_slice(raw);
        out.extend_from_slice(html);
        out
    }
}

impl_key!(Slug<CategoryKey>);
impl_key!(Slug<FileKey>);
impl_key!(Slug<UserKey>);
impl_key!(EntryId);
