mod auth;
mod edit;
mod upload;

pub use auth::*;
pub use edit::*;
pub use upload::*;

// util

use axum::Json;
use axum::http::StatusCode;

/// create a 500 internal server error response
fn internal_error(e: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
}

/// create an error response with status code
fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": msg.to_string()})))
}

// home

use axum::extract::State;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::model::user::Session;
use book::model::{AppState, PageContext, error::AppError};
use book::model::{FILES, PAGE_HTML, PAGES};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

/// show home page with pages and files
pub async fn home_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let mut pages = Vec::new();
    for result in pages_table.iter()? {
        let (key, value) = result?;
        let date = OffsetDateTime::from_unix_timestamp(value.value().date())
            .ok()
            .and_then(|d| d.format(&Iso8601::DATE).ok())
            .unwrap_or_default();
        pages.push(serde_json::json!({
            "name": key.value(),
            "title": value.value().title,
            "date": date,
        }));
    }

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("pages", &pages)
        .insert("user", &user);
    Ok(Html(page.render("home.html")?))
}

// view

use axum::extract::Path;

/// show a single page
pub async fn view_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let Some(meta) = pages_table.get(slug.as_str())? else {
        return Err(AppError::new(
            axum::http::StatusCode::NOT_FOUND,
            format!("page not found: {slug}"),
        ));
    };

    let html_table = tx.open_table(PAGE_HTML)?;
    let Some(body) = html_table.get(slug.as_str())? else {
        return Err(AppError::new(
            axum::http::StatusCode::NOT_FOUND,
            format!("page body not found: {slug}"),
        ));
    };

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let meta = meta.value();
    let date = OffsetDateTime::from_unix_timestamp(meta.date())
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let page = PageContext::new()
        .insert("page_title", &meta.title)
        .insert("content", &body.value())
        .insert("user", &user)
        .insert("slug", &slug)
        .insert("page_date", &date)
        .insert("page_creator", &meta.creator);
    Ok(Html(page.render("view.html")?))
}

// profile

/// show profile page
pub async fn profile_page(
    jar: CookieJar,
    book::model::user::UserToken(token): book::model::user::UserToken,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let invite = book::model::user::Invitation::new(&token?);
    let invitation = Signed::new(invite.clone());
    let code = invitation.generate(&CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(invite.expires_at)
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let invite_url = format!("{}/sign-up?invite={}", CONFIG.base_url, code);

    let page = PageContext::new()
        .insert("page_title", "Profile")
        .insert("user", &user)
        .insert("invite_url", &invite_url)
        .insert("invite_code_expiry", &expires_at);
    Ok(Html(page.render("profile.html")?))
}

// download

use axum::response::IntoResponse;
use book::model::FILE_BLOB;

/// download a file by slug
pub async fn file_download(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<(StatusCode, impl IntoResponse), AppError> {
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(meta) = files_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {slug}"),
        ));
    };
    let filename = meta.value().title.clone();
    drop(files_table);

    let blobs_table = tx.open_table(FILE_BLOB)?;
    let Some(blob) = blobs_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file blob not found: {slug}"),
        ));
    };
    let data = blob.value();
    drop(blobs_table);

    let content_type =
        mime_guess::from_path(&filename).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let headers = [
        ("Content-Type", content_type.to_string()),
        (
            "Content-Disposition",
            format!("inline; filename=\"{}\"", filename),
        ),
    ];

    Ok((StatusCode::OK, (headers, data)))
}
