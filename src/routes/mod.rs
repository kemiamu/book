use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::{AppState, ENTRIES, ENTRY_BODY, EntryMeta, FILES};
use book::model::{FILE_BLOB, PageContext, Passkey, Session, Slug, UserToken};
use redb::{ReadableDatabase, ReadableTable};
use std::collections::HashSet;
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
//
// ++++++++++++============++++++++++++============++++++++++++============

/// delete an entry and all its files
pub async fn entry_delete(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let slug = Slug::new(slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let tx = state.db.begin_write()?;

    // collect all file keys for this entry
    let files_to_remove: Vec<(Slug, Slug)> = {
        let files_table = tx.open_table(FILES)?;
        let mut keys = Vec::new();
        for result in files_table.iter()? {
            let (key, _) = result?;
            let (entry, file) = key.value();
            if entry == slug {
                keys.push((entry, file));
            }
        }
        keys
    };

    // remove file blobs
    {
        let mut blobs_table = tx.open_table(FILE_BLOB)?;
        for key in &files_to_remove {
            blobs_table.remove(key)?;
        }
    }

    // remove file metadata
    {
        let mut files_table = tx.open_table(FILES)?;
        for key in &files_to_remove {
            files_table.remove(key)?;
        }
    }

    // remove entry data from all tables
    {
        let mut entries_table = tx.open_table(ENTRIES)?;
        entries_table.remove(&slug)?;
    }
    {
        let mut body_table = tx.open_table(ENTRY_BODY)?;
        body_table.remove(&slug)?;
    }

    tx.commit()?;

    Ok(Json(serde_json::json!({"redirect": "/"})))
}

// util
//
// ++++++++++++============++++++++++++============++++++++++++============

/// create a 500 internal server error response
fn internal_error<E: ToString>(error: E) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": error.to_string()})),
    )
}

/// create an error response with status code
fn err<M: ToString>(status: StatusCode, msg: M) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": msg.to_string()})))
}

// home
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show home page with entries
pub async fn home_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries: Vec<(String, EntryMeta)> = Vec::new();
    let mut tag_set: HashSet<String> = HashSet::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let meta = value.value();
        for tag in &meta.tags {
            tag_set.insert(tag.to_string());
        }
        entries.push((key.value().to_string(), meta));
    }

    // most recently updated first
    entries.sort_by(|a, b| b.1.last_modified.cmp(&a.1.last_modified));

    let mapped = entries.into_iter().map(|(name, meta)| {
        serde_json::json!({
            "href": format!("/{name}/README.md"),
            "title": meta.title,
        })
    });
    let entries: Vec<serde_json::Value> = mapped.collect();

    let mut tags: Vec<String> = tag_set.into_iter().collect();
    tags.sort();
    let tags: Vec<serde_json::Value> = tags
        .into_iter()
        .map(|tag| {
            serde_json::json!({
                "href": format!("/tags/{tag}/README.md"),
                "title": tag,
            })
        })
        .collect();

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("entries", &entries)
        .insert("tags", &tags)
        .insert("user", &user);
    Ok(Html(page.render("home.html")?))
}

// tags
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show entries filtered by a tag
pub async fn tags_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(tag): Path<String>,
) -> Result<Html<String>, AppError> {
    let tag = Slug::new(tag).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries: Vec<(String, EntryMeta)> = Vec::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let meta = value.value();
        if meta.tags.contains(&tag) {
            entries.push((key.value().to_string(), meta));
        }
    }

    // most recently updated first
    entries.sort_by(|a, b| b.1.last_modified.cmp(&a.1.last_modified));

    let mapped = entries.into_iter().map(|(name, meta)| {
        serde_json::json!({
            "href": format!("/{name}/README.md"),
            "title": meta.title,
        })
    });
    let entries: Vec<serde_json::Value> = mapped.collect();

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let page = PageContext::new()
        .insert("page_title", &tag)
        .insert("tag", &tag)
        .insert("entries", &entries)
        .insert("user", &user);
    Ok(Html(page.render("tags.html")?))
}

// view
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show an entry page
pub async fn entry_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let slug = Slug::new(slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let Some(row) = entries_table.get(&slug)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry not found: {slug}"),
        ));
    };

    let body_table = tx.open_table(ENTRY_BODY)?;
    let Some(body) = body_table.get(&slug)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry body not found: {slug}"),
        ));
    };

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let entry_meta = row.value();
    let date = OffsetDateTime::from_unix_timestamp(entry_meta.last_modified)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let page = PageContext::new()
        .insert("page_title", &entry_meta.title)
        .insert("content", &body.value().html)
        .insert("user", &user)
        .insert("slug", &slug)
        .insert("page_date", &date)
        .insert("page_editor", &entry_meta.editor)
        .insert("page_slug", &slug);
    Ok(Html(page.render("entry.html")?))
}

// profile
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show profile page
pub async fn profile_page(
    jar: CookieJar,
    UserToken(token): UserToken,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let passkey = Passkey::new(token?);
    let expires_at = passkey.expires_at();
    let signed = Signed::new(passkey);
    let code = signed.generate(&CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(expires_at)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
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
//
// ++++++++++++============++++++++++++============++++++++++++============

/// download a file by entry and file slug
pub async fn file_download(
    State(state): State<Arc<AppState>>,
    Path((entry_slug, file_slug)): Path<(String, String)>,
) -> Result<(StatusCode, impl IntoResponse), AppError> {
    let entry_slug =
        Slug::new(entry_slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let file_slug = Slug::new(file_slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let key = (entry_slug, file_slug);
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(_meta) = files_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {}/{}", key.0, key.1),
        ));
    };
    drop(files_table);

    let blobs_table = tx.open_table(FILE_BLOB)?;
    let Some(blob) = blobs_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file blob not found: {}/{}", key.0, key.1),
        ));
    };
    let data = blob.value();
    drop(blobs_table);

    let content_type =
        mime_guess::from_path(key.1.as_ref()).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let headers = [
        ("Content-Type", content_type.to_string()),
        (
            "Content-Disposition",
            format!("inline; filename=\"{}\"", key.1),
        ),
    ];

    Ok((StatusCode::OK, (headers, data)))
}
