use crate::CONFIG;
use crate::crypto::{Mac, Signable, Signed};
use crate::model::error::AppError;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;
use redb::TableDefinition;
use rkyv::{Archive, rancor};

/// users table definition
pub const USERS: TableDefinition<&str, User> = TableDefinition::new("users");

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
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

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
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
        rkyv::to_bytes::<rancor::Error>(self).unwrap().into_vec()
    }
    /// deserialize invitation from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Invitation, rancor::Error>(bytes).ok()
    }
}

// session

#[derive(Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone)]
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
        rkyv::to_bytes::<rancor::Error>(self).unwrap().into_vec()
    }
    /// deserialize session from bytes
    fn deserialize(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<Session, rancor::Error>(bytes).ok()
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

        let Some(session) = Signed::<Session>::parse(cookie.value(), &CONFIG.secret) else {
            return Ok(UserToken(Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Invalid or expired session",
            ))));
        };

        Ok(UserToken(Ok(session.inner.user)))
    }
}

// store

impl redb::Value for User {
    type SelfType<'a>
        = User
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn type_name() -> redb::TypeName {
        redb::TypeName::new("User")
    }

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        let owned = data.to_vec();
        rkyv::from_bytes::<User, rancor::Error>(&owned).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'b,
    {
        rkyv::to_bytes::<rancor::Error>(value).unwrap().to_vec()
    }
}
