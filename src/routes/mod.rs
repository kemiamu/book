use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::{AppState, PageContext, Passkey, Session, UserToken};
use book::model::{ENTRIES, ENTRY_HTML, ENTRY_RAW, FILE_BLOB, FILES};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

mod auth;
mod edit;
mod upload;

pub use auth::*;
pub use edit::*;
pub use upload::*;

// delete

/// delete an entry and all its files
pub async fn entry_delete(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let tx = state.db.begin_write()?;

    // collect all file keys for this entry
    let files_to_remove: Vec<(String, String)> = {
        let files_table = tx.open_table(FILES)?;
        let mut keys = Vec::new();
        for result in files_table.iter()? {
            let (key, _) = result?;
            let (entry, file) = key.value();
            if entry == slug.as_str() {
                keys.push((entry.to_string(), file.to_string()));
            }
        }
        keys
    };

    // remove file blobs
    {
        let mut blobs_table = tx.open_table(FILE_BLOB)?;
        for (entry, file) in &files_to_remove {
            blobs_table.remove((entry.as_str(), file.as_str()))?;
        }
    }

    // remove file metadata
    {
        let mut files_table = tx.open_table(FILES)?;
        for (entry, file) in &files_to_remove {
            files_table.remove((entry.as_str(), file.as_str()))?;
        }
    }

    // remove entry data from all tables
    {
        let mut entries_table = tx.open_table(ENTRIES)?;
        entries_table.remove(slug.as_str())?;
    }
    {
        let mut raw_table = tx.open_table(ENTRY_RAW)?;
        raw_table.remove(slug.as_str())?;
    }
    {
        let mut html_table = tx.open_table(ENTRY_HTML)?;
        html_table.remove(slug.as_str())?;
    }

    tx.commit()?;

    Ok(Json(serde_json::json!({"redirect": "/"})))
}

// util

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

/// show home page with entries
pub async fn home_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries = Vec::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let entry_meta = value.value();
        entries.push(serde_json::json!({
            "name": key.value(),
            "title": entry_meta.title,
        }));
    }

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("entries", &entries)
        .insert("user", &user);
    Ok(Html(page.render("home.html")?))
}

// view

/// show an entry page
pub async fn entry_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let Some(row) = entries_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry not found: {slug}"),
        ));
    };

    let html_table = tx.open_table(ENTRY_HTML)?;
    let Some(body) = html_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry body not found: {slug}"),
        ));
    };

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let entry_meta = row.value();
    let date = OffsetDateTime::from_unix_timestamp(entry_meta.last_modified)
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let page = PageContext::new()
        .insert("page_title", &entry_meta.title)
        .insert("content", &body.value())
        .insert("user", &user)
        .insert("slug", &slug)
        .insert("page_date", &date)
        .insert("page_editor", &entry_meta.editor)
        .insert("entry_slug", &slug);
    Ok(Html(page.render("entry.html")?))
}

// profile

/// show profile page
pub async fn profile_page(
    jar: CookieJar,
    UserToken(token): UserToken,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let passkey = Passkey::new(&token?);
    let signed = Signed::new(passkey.clone());
    let code = signed.generate(&CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(passkey.expires_at)
        .ok()
        .and_then(|d| d.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let passkey_url = format!("{}/auth?passkey={}", CONFIG.base_url, code);

    let page = PageContext::new()
        .insert("page_title", "Profile")
        .insert("user", &user)
        .insert("passkey_url", &passkey_url)
        .insert("passkey_code_expiry", &expires_at);
    Ok(Html(page.render("profile.html")?))
}

// download

/// download a file by entry and file slug
pub async fn file_download(
    State(state): State<Arc<AppState>>,
    Path((entry_slug, file_slug)): Path<(String, String)>,
) -> Result<(StatusCode, impl IntoResponse), AppError> {
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let key = (entry_slug.as_str(), file_slug.as_str());
    let Some(_meta) = files_table.get(key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {entry_slug}/{file_slug}"),
        ));
    };
    drop(files_table);

    let blobs_table = tx.open_table(FILE_BLOB)?;
    let Some(blob) = blobs_table.get(key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file blob not found: {entry_slug}/{file_slug}"),
        ));
    };
    let data = blob.value();
    drop(blobs_table);

    let content_type =
        mime_guess::from_path(&file_slug).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let headers = [
        ("Content-Type", content_type.to_string()),
        (
            "Content-Disposition",
            format!("inline; filename=\"{}\"", file_slug),
        ),
    ];

    Ok((StatusCode::OK, (headers, data)))
}
