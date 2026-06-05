use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use book::CONFIG;
use book::crypto::Signed;
use book::model::res::{FILES, PAGE_BODIES, PAGES};
use book::model::user::{Invitation, Session, USERS, User, UserToken};
use book::model::{AppState, PageContext, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};

// home

pub async fn home_page(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let mut pages: Vec<(String, String)> = Vec::new();
    for result in pages_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        pages.push((key.value().to_string(), r.title.to_string()));
    }

    let files_table = tx.open_table(FILES)?;
    let mut files: Vec<(String, String)> = Vec::new();
    for result in files_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        files.push((key.value().to_string(), r.title.to_string()));
    }

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("pages", &pages)
        .insert("files", &files);
    Ok(Html(page.render("home.html")?))
}

// view

pub async fn view_page(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let Some(meta) = pages_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("page not found: {slug}"),
        ));
    };

    let bodies_table = tx.open_table(PAGE_BODIES)?;
    let Some(body) = bodies_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("page body not found: {slug}"),
        ));
    };

    let page = PageContext::new()
        .insert("page_title", &meta.value().title)
        .insert("content", &body.value().render());
    Ok(Html(page.render("page.html")?))
}

// sign in

pub async fn sign_in_page() -> Result<Html<String>, AppError> {
    let page = PageContext::new().insert("page_title", "Sign In");
    Ok(Html(page.render("sign-in.html")?))
}

#[derive(Deserialize)]
pub struct SignInForm {
    username: String,
    password: String,
}

pub async fn sign_in_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignInForm>,
) -> Result<impl IntoResponse, AppError> {
    let tx = state.db.begin_read()?;
    let table = tx.open_table(USERS)?;

    let user = table
        .get(body.username.as_str())?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid username or password"))?
        .value();

    if !user.verify(&body.password, &CONFIG.secret) {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign up

pub async fn sign_up_page(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let invite = params.get("invite").cloned();

    let page = PageContext::new()
        .insert("page_title", "Sign Up")
        .insert("invite", &invite);
    Ok(Html(page.render("sign-up.html")?))
}

#[derive(Deserialize)]
pub struct SignUpForm {
    username: String,
    password: String,
    invite: String,
}

pub async fn sign_up_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignUpForm>,
) -> Result<impl IntoResponse, AppError> {
    let invitation = Signed::<Invitation>::parse(&body.invite, &CONFIG.secret)
        .ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "Invalid or expired invite code"))?;

    let tx = state.db.begin_write()?;
    let mut table = tx.open_table(USERS)?;

    if table.get(body.username.as_str())?.is_some() {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "Username already exists",
        ));
    }

    let user = User::new(&body.password, &CONFIG.secret, invitation.inner.inviter);
    table.insert(body.username.as_str(), user)?;
    drop(table);
    tx.commit()?;

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// profile

pub async fn profile_page(UserToken(_token): UserToken) -> Result<Html<String>, AppError> {
    let page = PageContext::new().insert("page_title", "Profile");
    Ok(Html(page.render("profile.html")?))
}

pub async fn generate_invite(
    UserToken(token): UserToken,
) -> Result<Json<serde_json::Value>, AppError> {
    let invitation = Signed::new(Invitation::new(&token?));
    let code = invitation.generate(&CONFIG.secret);

    Ok(Json(serde_json::json!({"code": code})))
}

// helper

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
