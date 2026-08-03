use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::{AppState, EntryBody, EntryMeta, Markdown, PageContext, Slug};
use book::model::{ENTRIES, ENTRY_BODY};
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
    let (slug, title, body) = if let Some(entry) = &params.entry {
        let entry_slug = Slug::new(entry.clone()).map_err(|e| {
            AppError::new(StatusCode::BAD_REQUEST, format!("invalid entry slug: {e}"))
        })?;
        let tx = state.db.begin_read()?;

        let entries_table = tx.open_table(ENTRIES)?;
        let meta = entries_table.get(entry_slug.clone())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry not found: {entry_slug}"),
            )
        })?;

        let bodies_table = tx.open_table(ENTRY_BODY)?;
        let body = bodies_table.get(entry_slug.clone())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry body not found: {entry_slug}"),
            )
        })?;

        (
            entry_slug.to_string(),
            meta.value().title.clone(),
            body.value().raw.into_inner(),
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

    let slug =
        Slug::new(body.slug.clone()).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;

    // a fresh entry can be created with just a slug; fall back to the slug
    // as the title until a real one is provided
    let title = if body.title.is_empty() {
        slug.as_ref()
    } else {
        body.title.as_str()
    };

    let mut entries_table = tx.open_table(ENTRIES)?;
    let existing = entries_table.get(slug.clone())?.map(|g| g.value());
    let meta = EntryMeta::new(
        title,
        &username,
        existing.map(|m| m.tags).unwrap_or_default(),
    );
    entries_table.insert(slug.clone(), meta)?;
    drop(entries_table);

    let md = Markdown::new(body.body.clone());

    let mut body_table = tx.open_table(ENTRY_BODY)?;
    body_table.insert(slug, EntryBody::new(md))?;
    drop(body_table);

    tx.commit()?;

    Ok(Json(serde_json::json!({})))
}
