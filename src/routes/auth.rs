use axum::response::{Html, IntoResponse, Redirect};
use axum::{Json, extract::Query, extract::State, http::HeaderMap, http::StatusCode};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use book::model::{AppState, PageContext, Passkey, Session, Slug, USERS, User, UserKey};
use book::{CONFIG, crypto::Signed, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

/// show auth page (login + register unified)
pub async fn auth_page(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let code = params.get("passkey").cloned().unwrap_or_default();
    let valid = Signed::<Passkey>::parse(&code, &CONFIG.secret).is_some();

    if !valid {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid or expired passkey",
        ));
    }

    let page = PageContext::new()
        .insert("page_title", "Authorization")
        .insert("passkey", &code)
        .insert("passkey_valid", &valid.to_string());
    Ok(Html(page.render("auth.html")?))
}

// sign in
//
// ++++++++++++============++++++++++++============++++++++++++============

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
) -> Result<(CookieJar, Json<serde_json::Value>), AppError> {
    let _passkey = Signed::<Passkey>::parse(&body.passkey, &CONFIG.secret)
        .ok_or_else(|| AppError::json(StatusCode::UNAUTHORIZED, "Invalid or expired passkey"))?;

    let username =
        Slug::new(body.username).map_err(|e| AppError::json(StatusCode::BAD_REQUEST, e))?;

    let tx = state.db.begin_read()?;
    let table = tx.open_table(USERS)?;

    let user = table
        .get(&username)?
        .ok_or_else(|| AppError::json(StatusCode::UNAUTHORIZED, "Invalid username or password"))?
        .value();

    if !user.verify(&body.password, &CONFIG.secret) {
        return Err(AppError::json(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let jar = set_session_cookie(jar, username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign up
//
// ++++++++++++============++++++++++++============++++++++++++============

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
) -> Result<(CookieJar, Json<serde_json::Value>), AppError> {
    let passkey = Signed::<Passkey>::parse(&body.passkey, &CONFIG.secret)
        .ok_or_else(|| AppError::json(StatusCode::UNAUTHORIZED, "Invalid or expired passkey"))?;

    let username =
        Slug::new(body.username).map_err(|e| AppError::json(StatusCode::BAD_REQUEST, e))?;

    let tx = state.db.begin_write()?;
    let mut table = tx.open_table(USERS)?;

    if table.get(&username)?.is_some() {
        return Err(AppError::json(
            StatusCode::CONFLICT,
            "Username already exists",
        ));
    }

    let user = User::new(&body.password, &CONFIG.secret, passkey.inner.into_creator());
    table.insert(&username, user)?;
    drop(table);
    tx.commit()?;

    let jar = set_session_cookie(jar, username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign out
//
// ++++++++++++============++++++++++++============++++++++++++============

/// handle sign-out and clear session
pub async fn sign_out(jar: CookieJar, headers: HeaderMap) -> impl IntoResponse {
    let jar = jar.remove(Cookie::build(("session", "")).path("/").build());
    let dest = headers
        .get("Referer")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("/");
    (jar, Redirect::to(dest))
}

// helper
//
// ++++++++++++============++++++++++++============++++++++++++============

/// set session cookie on the jar
fn set_session_cookie(jar: CookieJar, username: Slug<UserKey>, secret: &str) -> CookieJar {
    let token = Signed::new(Session::new(username)).generate(secret);
    let cookie = Cookie::build(("session", token))
        .path("/")
        .max_age(time::Duration::seconds(Session::EXPIRY_SECS))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build();
    jar.add(cookie)
}
