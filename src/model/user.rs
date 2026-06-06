use crate::crypto::{Mac, Signable, Signed};
use crate::model::error::AppError;
use crate::{CONFIG, impl_stored};
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;

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

        let Some(session) = Signed::<Session>::parse(cookie.value(), &CONFIG.secret) else {
            return Ok(UserToken(Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Invalid or expired session",
            ))));
        };

        Ok(UserToken(Ok(session.inner.user)))
    }
}

impl_stored!(User);
