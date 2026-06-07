use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::{AppState, EntryMeta, Markdown, PageContext};
use book::model::{ENTRIES, ENTRY_HTML, ENTRY_RAW};
use book::model::{Session, UserToken};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;

use std::sync::Arc;

#[derive(Deserialize)]
/// edit entry query params
pub struct EditQuery {
    pub entry: Option<String>,
}

/// show edit entry page
pub async fn edit_page(
    jar: CookieJar,
    _token: UserToken,
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditQuery>,
) -> Result<Html<String>, AppError> {
    let (slug, title, body) = if let Some(ref entry_slug) = params.entry {
        let tx = state.db.begin_read()?;

        let entries_table = tx.open_table(ENTRIES)?;
        let meta = entries_table.get(entry_slug.as_str())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry not found: {entry_slug}"),
            )
        })?;

        let bodies_table = tx.open_table(ENTRY_RAW)?;
        let body = bodies_table.get(entry_slug.as_str())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry body not found: {entry_slug}"),
            )
        })?;

        (
            entry_slug.clone(),
            meta.value().title.clone(),
            body.value().into_inner(),
        )
    } else {
        (String::new(), String::new(), String::new())
    };

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);
    let page = PageContext::new()
        .insert("page_title", "Edit")
        .insert("slug", &slug)
        .insert("title", &title)
        .insert("body", &body)
        .insert("error", "")
        .insert("user", &user);
    Ok(Html(page.render("edit.html")?))
}

#[derive(Deserialize)]
/// edit form payload
pub struct EditForm {
    pub slug: String,
    pub title: String,
    pub body: String,
}

/// handle page save
pub async fn edit_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Json(body): Json<EditForm>,
) -> Result<Json<serde_json::Value>, AppError> {
    let tx = state.db.begin_write()?;

    let username = token?;

    if body.slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Slug must not be empty",
        ));
    }
    if body.title.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Title must not be empty",
        ));
    }

    let mut entries_table = tx.open_table(ENTRIES)?;
    let existing = entries_table.get(body.slug.as_str())?.map(|g| g.value());
    let meta = EntryMeta::new(
        &body.title,
        &username,
        existing.map(|m| m.tags).unwrap_or_default(),
    );
    entries_table.insert(body.slug.as_str(), meta)?;
    drop(entries_table);

    let md = Markdown::new(body.body.clone());
    let html = md.render();

    let mut raw_table = tx.open_table(ENTRY_RAW)?;
    raw_table.insert(body.slug.as_str(), md)?;
    drop(raw_table);

    let mut html_table = tx.open_table(ENTRY_HTML)?;
    html_table.insert(body.slug.as_str(), html)?;
    drop(html_table);

    tx.commit()?;

    Ok(Json(serde_json::json!({})))
}
