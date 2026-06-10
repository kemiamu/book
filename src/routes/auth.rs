use super::{err, internal_error};
use axum::Json;
use axum::extract::Query;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::PageContext;
use book::model::USERS;
use book::model::{AppState, Passkey, Session, User};
use redb::ReadableDatabase;
use redb::ReadableTable;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

/// show auth page (login + register unified)
pub async fn auth_page(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let code = params.get("passkey").cloned().unwrap_or_default();
    let valid = Signed::<Passkey>::parse(&code, &CONFIG.secret).is_some();

    let page = PageContext::new()
        .insert("page_title", "Authorization")
        .insert("passkey", &code)
        .insert("passkey_valid", &valid);
    Ok(Html(page.render("auth.html")?))
}

// sign in

#[derive(Deserialize)]
/// sign-in form payload
pub struct SignInForm {
    pub passkey: String,
    pub username: String,
    pub password: String,
}

/// handle sign-in form submission
pub async fn sign_in_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignInForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let _passkey = Signed::<Passkey>::parse(&body.passkey, &CONFIG.secret)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid or expired passkey"))?;

    let tx = state.db.begin_read().map_err(internal_error)?;
    let table = tx.open_table(USERS).map_err(internal_error)?;

    let user = table
        .get(body.username.as_str())
        .map_err(internal_error)?
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid username or password"))?
        .value();

    if !user.verify(&body.password, &CONFIG.secret) {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign up

#[derive(Deserialize)]
/// sign-up form payload
pub struct SignUpForm {
    pub passkey: String,
    pub username: String,
    pub password: String,
}

/// handle sign-up form submission
pub async fn sign_up_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignUpForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let passkey = Signed::<Passkey>::parse(&body.passkey, &CONFIG.secret)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid or expired passkey"))?;

    let tx = state.db.begin_write().map_err(internal_error)?;
    let mut table = tx.open_table(USERS).map_err(internal_error)?;

    if table
        .get(body.username.as_str())
        .map_err(internal_error)?
        .is_some()
    {
        return Err(err(StatusCode::CONFLICT, "Username already exists"));
    }

    let user = User::new(&body.password, &CONFIG.secret, passkey.inner.creator);
    table
        .insert(body.username.as_str(), user)
        .map_err(internal_error)?;
    drop(table);
    tx.commit().map_err(internal_error)?;

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign out

/// handle sign-out and clear session
pub async fn sign_out(jar: CookieJar, headers: HeaderMap) -> impl IntoResponse {
    let jar = jar.remove(Cookie::from("session"));
    let dest = headers
        .get("Referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");
    (jar, Redirect::to(dest))
}

// helper

/// set session cookie on the jar
fn set_session_cookie(jar: CookieJar, username: impl AsRef<str>, secret: &str) -> CookieJar {
    let token = Signed::new(Session::new(username.as_ref())).generate(secret);
    let cookie = Cookie::build(("session", token))
        .path("/")
        .max_age(time::Duration::seconds(Session::EXPIRY_SECS))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build();
    jar.add(cookie)
}
