//! canonical definitions of the application's fundamental data formats

use crate::crypto::Signed;
use redb::{ReadableTable, TableDefinition as Table};
use std::path::Path;

pub use auth::*;
pub use key::*;
pub use resource::*;

// schema & state
//
// ++++++++++++============++++++++++++============++++++++++++============

/// entries table definition, keyed by hex entry id
pub const ENTRIES: Table<EntryId, EntryMeta> = Table::new("entries");
/// entry body table definition, markdown with its rendered html cache
pub const ENTRY_BODY: Table<EntryId, Markdown<'static>> = Table::new("entry_body");
/// singleton counter that allocates entry ids
pub const ENTRY_COUNTER: Table<(), u64> = Table::new("entry_counter");

/// files table definition, keyed by (entry id, file name)
pub const FILES: Table<(EntryId, Slug<FileName>), FileMeta> = Table::new("files");
/// file blob table definition, borrowed slices for zero-copy reads
pub const FILE_BLOB: Table<(EntryId, Slug<FileName>), &[u8]> = Table::new("file_blob");

/// users table definition
pub const USERS: Table<Slug<UserName>, User> = Table::new("users");

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
    let next = table.get(())?.map_or(0, |n| n.value() + 1);
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
        ctx.insert("copyright", &crate::CONFIG.copyright);
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

// key types
//
// ++++++++++++============++++++++++++============++++++++++++============

mod key {
    use std::{borrow::Cow, marker::PhantomData};

    /// hex-encoded entry id, allocated from the singleton entry counter
    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    #[repr(transparent)]
    pub struct EntryId(String);

    impl From<u64> for EntryId {
        /// format a counter value as lowercase hex
        fn from(counter: u64) -> Self {
            Self(format!("{counter:x}"))
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

    /// validated slug: non-empty, single URL path segment, tagged with its rule
    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(bound(serialize = ""), transparent)]
    #[repr(transparent)]
    pub struct Slug<T: SlugRule>(Cow<'static, str>, PhantomData<T>);

    impl<T: SlugRule> Slug<T> {
        /// validate and create a new slug
        pub fn new<V: Into<Cow<'static, str>>>(value: V) -> Option<Self> {
            let cow = value.into();
            match cow.is_empty() || cow.len() > T::MAX_LEN || !cow.chars().all(T::is_allowed) {
                true => None,
                false => Some(Self(cow, PhantomData)),
            }
        }

        /// split a string on disallowed characters into valid slugs
        pub fn split(input: &str) -> impl Iterator<Item = Slug<T>> {
            input
                .split(|ch: char| !T::is_allowed(ch))
                .filter_map(|seg| Slug::new(seg.to_owned()))
        }

        /// convert a raw name into a valid slug, replacing runs of
        /// disallowed characters with '-'
        pub fn normalize(input: &str) -> Option<Slug<T>> {
            let mut parts = Self::split(input);
            let mut out = parts.next()?.to_string();
            for part in parts {
                out.push('-');
                out.push_str(part.as_ref());
            }
            Slug::new(out)
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

    /// validation rules for a kind of slug: max length + allowed characters
    pub trait SlugRule: std::fmt::Debug {
        /// maximum length in bytes
        const MAX_LEN: usize;

        /// whether a character is allowed
        fn is_allowed(ch: char) -> bool;
    }

    /// define a slug marker type with its validation rule
    macro_rules! slug_key {
        ($name:ident, $len:expr, $($ch:literal)|+) => {
            #[derive(Debug, Clone, PartialEq, Eq)]
            pub struct $name;

            impl SlugRule for $name {
                const MAX_LEN: usize = $len;

                fn is_allowed(ch: char) -> bool {
                    ch.is_ascii_alphanumeric() || matches!(ch, $($ch)|+)
                }
            }
        };
    }

    slug_key!(FileName, 255, '-' | '_' | '.');
    slug_key!(UserName, 32, '-' | '_');
}

// resource types
//
// ++++++++++++============++++++++++++============++++++++++++============

mod resource {
    use super::key::{Slug, UserName};
    use crate::html::HtmlWriter;
    use std::borrow::Cow;

    /// file metadata
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct FileMeta {
        pub editor: Slug<UserName>,
        pub last_modified: i64,
    }

    impl FileMeta {
        /// create new resource metadata with current timestamp
        pub fn new<E: Into<Slug<UserName>>>(editor: E) -> Self {
            Self {
                editor: editor.into(),
                last_modified: time::UtcDateTime::now().unix_timestamp(),
            }
        }
    }

    /// metadata for entries
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct EntryMeta {
        pub title: String,
        pub category: String,
        pub editor: Slug<UserName>,
        pub created_at: i64,
        pub last_modified: i64,
    }

    impl EntryMeta {
        /// create new entry metadata
        pub fn new<T: Into<String>, C: Into<String>, E: Into<Slug<UserName>>>(
            title: T,
            category: C,
            editor: E,
        ) -> Self {
            let now = time::UtcDateTime::now().unix_timestamp();
            Self {
                title: title.into(),
                category: category.into(),
                editor: editor.into(),
                created_at: now,
                last_modified: now,
            }
        }

        /// update metadata, preserving the creation time
        pub fn update<T: Into<String>, C: Into<String>, E: Into<Slug<UserName>>>(
            &self,
            title: T,
            category: C,
            editor: E,
        ) -> Self {
            Self {
                title: title.into(),
                category: category.into(),
                editor: editor.into(),
                created_at: self.created_at,
                last_modified: time::UtcDateTime::now().unix_timestamp(),
            }
        }
    }

    /// markdown content with its rendered html cache
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Markdown<'a> {
        #[serde(borrow)]
        pub(crate) raw: Cow<'a, str>,
        #[serde(borrow)]
        pub(crate) html: Cow<'a, str>,
    }

    impl<'a> Markdown<'a> {
        /// create markdown from string, rendering the html cache immediately
        pub fn new<C: Into<Cow<'a, str>>>(content: C) -> Self {
            let raw = content.into();
            let html = Self::render(&raw);
            Self {
                raw,
                html: Cow::Owned(html),
            }
        }

        /// borrow the raw markdown text
        pub fn raw(&self) -> &str {
            &self.raw
        }

        /// borrow the rendered html
        pub fn html(&self) -> &str {
            &self.html
        }

        /// get the raw markdown text
        pub fn into_inner(self) -> String {
            self.raw.into_owned()
        }

        /// render markdown to html
        fn render(raw: &str) -> String {
            use pulldown_cmark as markdown;
            let parser = markdown::Parser::new_ext(raw, markdown::Options::all());
            let mut html_output: String = Default::default();
            HtmlWriter::new(parser, &mut html_output).run().unwrap();
            html_output
        }
    }
}

// auth
//
// ++++++++++++============++++++++++++============++++++++++++============

mod auth {
    use super::key::{Slug, UserName};
    use crate::{crypto::Mac, crypto::Signable, crypto::Signed, error::AppError};
    use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
    use axum_extra::extract::cookie::CookieJar;

    /// a registered user
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct User {
        password: Mac,
        pub parent: Option<Slug<UserName>>,
    }

    impl User {
        const PASSWD_TAG: &str = "password";

        /// create a new user
        pub fn new<P: AsRef<[u8]>, S: AsRef<[u8]>>(
            password: P,
            secret: S,
            parent: Option<Slug<UserName>>,
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

    /// authorization passkey token
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Passkey {
        creator: Option<Slug<UserName>>,
        expires_at: i64,
    }

    impl Passkey {
        pub const EXPIRY_SECS: i64 = 7 * 24 * 60 * 60;
        const BOOTSTRAP_EXPIRY_SECS: i64 = 24 * 60 * 60;

        /// create an invitation passkey for a known user
        pub fn new<C: Into<Slug<UserName>>>(creator: C) -> Self {
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
        pub fn creator(&self) -> Option<&Slug<UserName>> {
            self.creator.as_ref()
        }

        /// consume the passkey, returning its creator
        pub fn into_creator(self) -> Option<Slug<UserName>> {
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

    /// user session token
    #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
    pub struct Session {
        pub user: Slug<UserName>,
        pub expires_at: i64,
    }

    impl Session {
        pub const EXPIRY_SECS: i64 = 3650 * 24 * 60 * 60;

        /// create a new session
        pub fn new<U: Into<Slug<UserName>>>(user: U) -> Self {
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
    pub struct UserToken(pub Result<Slug<UserName>, AppError>);

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

            let Some(session) = Signed::<Session>::parse(cookie.value(), &crate::CONFIG.secret)
            else {
                return Ok(UserToken(Err(AppError::new(
                    StatusCode::UNAUTHORIZED,
                    "Invalid or expired session",
                ))));
            };

            Ok(UserToken(Ok(session.inner.user)))
        }
    }
}

// store
//
// ++++++++++++============++++++++++++============++++++++++++============

mod store {
    use super::auth::User;
    use super::key::{EntryId, FileName, Slug, UserName};
    use super::resource::{EntryMeta, FileMeta, Markdown};

    /// implement redb::Value via postcard for a serde type.
    /// for a borrowed type pass the SelfType with the impl-block
    /// lifetime, e.g. `impl_stored!(Markdown<'static>, Markdown<'a>, "Markdown")`
    macro_rules! impl_stored {
        ($ty:ty) => {
            impl_stored!($ty, $ty, stringify!($ty));
        };
        ($ty:ty, $self:ty, $name:expr) => {
            impl redb::Value for $ty {
                type SelfType<'a>
                    = $self
                where
                    Self: 'a;
                type AsBytes<'a>
                    = Vec<u8>
                where
                    Self: 'a;

                fn type_name() -> redb::TypeName {
                    redb::TypeName::new($name)
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
    impl_stored!(User);
    impl_stored!(Markdown<'static>, Markdown<'a>, "Markdown");
    impl_stored!(EntryId);
    impl_stored!(Slug<FileName>);
    impl_stored!(Slug<UserName>);

    /// implement redb::Key from raw utf8 bytes, so byte order matches
    /// string order (the same encoding redb uses for `str` keys)
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

    impl_key!(EntryId);
    impl_key!(Slug<FileName>);
    impl_key!(Slug<UserName>);
}
