use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use book::model::res::{FILES, PAGE_BODIES, PAGES};
use book::model::user::{Invitation, Session, Signed, USERS, User};
use book::model::{AppError, AppState};
use book::{CONFIG, TEMPLATES};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;
use tera::Context;

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

    let mut ctx = Context::new();
    ctx.insert("site_title", &CONFIG.site_title);
    ctx.insert("base_url", &CONFIG.base_url);
    ctx.insert("page_title", "Home");
    ctx.insert("pages", &pages);
    ctx.insert("files", &files);

    let html = TEMPLATES.render("home.html", &ctx)?;
    Ok(Html(html))
}

pub async fn view_page(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let meta = pages_table
        .get(slug.as_str())?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("page not found: {slug}")))?;

    let bodies_table = tx.open_table(PAGE_BODIES)?;
    let body = bodies_table.get(slug.as_str())?.ok_or_else(|| {
        AppError::new(
            StatusCode::NOT_FOUND,
            format!("page body not found: {slug}"),
        )
    })?;

    let mut ctx = Context::new();
    ctx.insert("site_title", &CONFIG.site_title);
    ctx.insert("base_url", &CONFIG.base_url);
    ctx.insert("page_title", &meta.value().title);
    ctx.insert("content", &body.value().render());

    let html = TEMPLATES.render("page.html", &ctx)?;
    Ok(Html(html))
}

pub async fn sign_in_page() -> Result<Html<String>, AppError> {
    let mut ctx = Context::new();
    ctx.insert("site_title", &CONFIG.site_title);
    ctx.insert("base_url", &CONFIG.base_url);
    ctx.insert("page_title", "Sign In");

    let html = TEMPLATES.render("sign-in.html", &ctx)?;
    Ok(Html(html))
}

pub async fn sign_up_page(
    Query(params): Query<Vec<(String, String)>>,
) -> Result<Html<String>, AppError> {
    let invite = params
        .iter()
        .find(|(k, _)| k == "invite")
        .map(|(_, v)| v.clone());

    let mut ctx = Context::new();
    ctx.insert("site_title", &CONFIG.site_title);
    ctx.insert("base_url", &CONFIG.base_url);
    ctx.insert("page_title", "Sign Up");
    ctx.insert("invite", &invite);

    let html = TEMPLATES.render("sign-up.html", &ctx)?;
    Ok(Html(html))
}

// --- POST handlers ---

use serde::Deserialize;
use serde_json;

#[derive(Deserialize)]
pub(crate) struct AuthRequest {
    username: String,
    password: String,
}

fn set_session_cookie(username: &str, secret: &str) -> HeaderMap {
    let session = Signed::new(Session::new(username.to_string()));
    let token = session.generate(secret);
    let cookie = format!(
        "session={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax",
        token,
        Session::EXPIRY_SECS
    );
    let mut headers = HeaderMap::new();
    headers.insert("Set-Cookie", cookie.parse().unwrap());
    headers
}

pub async fn sign_in_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AuthRequest>,
) -> Result<impl IntoResponse, AppError> {
    let secret = &CONFIG.secret;

    let tx = state.db.begin_read()?;
    let table = tx.open_table(USERS)?;

    let user = table
        .get(body.username.as_str())?
        .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid username or password"))?
        .value();

    if !user.verify(&body.password, secret) {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let headers = set_session_cookie(&body.username, secret);
    Ok((headers, Json(serde_json::json!({}))))
}

#[derive(Deserialize)]
pub(crate) struct SignUpRequest {
    username: String,
    password: String,
    invite: String,
}

pub async fn sign_up_post(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignUpRequest>,
) -> Result<impl IntoResponse, AppError> {
    let secret = &CONFIG.secret;

    let invitation = Signed::<Invitation>::parse(&body.invite, secret)
        .ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "Invalid or expired invite code"))?;

    let tx = state.db.begin_write()?;
    let mut table = tx.open_table(USERS)?;

    if table.get(body.username.as_str())?.is_some() {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "Username already exists",
        ));
    }

    let user = User::new(&body.password, secret, invitation.inner.inviter);
    table.insert(body.username.as_str(), user)?;
    drop(table);
    tx.commit()?;

    let headers = set_session_cookie(&body.username, secret);
    Ok((headers, Json(serde_json::json!({}))))
}
