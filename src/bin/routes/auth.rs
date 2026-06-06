use super::{err, internal_error};
use axum::Json;
use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use book::CONFIG;
use book::crypto::Signed;
use book::model::user::{Invitation, Session, User};
use book::model::USERS;
use book::model::{PageContext, error::AppError};
use redb::ReadableDatabase;
use redb::ReadableTable;
use serde::Deserialize;
use std::collections::HashMap;

/// show sign-in page
pub async fn sign_in_page(jar: CookieJar) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Sign In")
        .insert("user", &user);
    Ok(Html(page.render("sign-in.html")?))
}

#[derive(Deserialize)]
/// sign-in form payload
pub struct SignInForm {
    pub username: String,
    pub password: String,
}

/// handle sign-in form submission
pub async fn sign_in_post(
    jar: CookieJar,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<book::model::AppState>>,
    Json(body): Json<SignInForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
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

/// show sign-up page
pub async fn sign_up_page(
    jar: CookieJar,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let invite = params.get("invite").cloned();

    let page = PageContext::new()
        .insert("page_title", "Sign Up")
        .insert("invite", &invite)
        .insert("user", &user);
    Ok(Html(page.render("sign-up.html")?))
}

#[derive(Deserialize)]
/// sign-up form payload
pub struct SignUpForm {
    pub username: String,
    pub password: String,
    pub invite: String,
}

/// handle sign-up form submission
pub async fn sign_up_post(
    jar: CookieJar,
    axum::extract::State(state): axum::extract::State<std::sync::Arc<book::model::AppState>>,
    Json(body): Json<SignUpForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let invitation = Signed::<Invitation>::parse(&body.invite, &CONFIG.secret)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "Invalid or expired invite code"))?;

    let tx = state.db.begin_write().map_err(internal_error)?;
    let mut table = tx.open_table(USERS).map_err(internal_error)?;

    if table
        .get(body.username.as_str())
        .map_err(internal_error)?
        .is_some()
    {
        return Err(err(StatusCode::CONFLICT, "Username already exists"));
    }

    let user = User::new(&body.password, &CONFIG.secret, invitation.inner.inviter);
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
