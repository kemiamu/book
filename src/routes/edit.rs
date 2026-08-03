use axum::{Json, extract::Query, extract::State, http::StatusCode, response::Html};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, ENTRIES, ENTRY_BODY, EntryBody, EntryMeta};
use book::model::{Markdown, PageContext, Session, Slug, UserToken};
use book::{CONFIG, crypto::Signed, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
/// edit entry query params
pub struct EditQuery {
    pub category: Option<String>,
    pub entry: Option<String>,
}

/// show edit entry page
pub async fn edit_page(
    jar: CookieJar,
    _token: UserToken,
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditQuery>,
) -> Result<Html<String>, AppError> {
    let (category, entry, title, body) =
        if let (Some(category), Some(entry)) = (&params.category, &params.entry) {
            let category = Slug::new(category.clone()).map_err(|e| {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("invalid category slug: {e}"),
                )
            })?;
            let entry = Slug::new(entry.clone()).map_err(|e| {
                AppError::new(StatusCode::BAD_REQUEST, format!("invalid entry slug: {e}"))
            })?;
            let key = (category, entry);
            let tx = state.db.begin_read()?;

            let entries_table = tx.open_table(ENTRIES)?;
            let meta = entries_table.get(&key)?.ok_or_else(|| {
                AppError::new(
                    StatusCode::NOT_FOUND,
                    format!("entry not found: {}/{}", key.0, key.1),
                )
            })?;
            let title = meta.value().title;

            let bodies_table = tx.open_table(ENTRY_BODY)?;
            let body = bodies_table.get(&key)?.ok_or_else(|| {
                AppError::new(
                    StatusCode::NOT_FOUND,
                    format!("entry body not found: {}/{}", key.0, key.1),
                )
            })?;

            (
                key.0.to_string(),
                key.1.to_string(),
                title,
                body.value().raw.into_inner(),
            )
        } else {
            (String::new(), String::new(), String::new(), String::new())
        };

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);
    let page = PageContext::new()
        .insert("page_title", "Edit")
        .insert("category", &category)
        .insert("entry", &entry)
        .insert("title", &title)
        .insert("body", &body)
        .insert("error", "")
        .insert("user", &user);
    Ok(Html(page.render("edit.html")?))
}

#[derive(Deserialize)]
/// edit form payload
pub struct EditForm {
    pub category: String,
    pub entry: String,
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

    let category =
        Slug::new(body.category).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let entry = Slug::new(body.entry).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let key = (category, entry);

    // a fresh entry can be created with just an entry slug; fall back to it
    // as the title until a real one is provided
    let title = if body.title.is_empty() {
        key.1.as_ref()
    } else {
        body.title.as_str()
    };

    let mut entries_table = tx.open_table(ENTRIES)?;
    let meta = match entries_table.get(&key)? {
        // keep the creation time when saving an existing entry
        Some(existing) => existing.value().update(title, username),
        None => EntryMeta::new(title, username),
    };
    entries_table.insert(&key, meta)?;
    drop(entries_table);

    let md = Markdown::new(body.body);

    let mut body_table = tx.open_table(ENTRY_BODY)?;
    body_table.insert(&key, EntryBody::new(md))?;
    drop(body_table);

    tx.commit()?;

    Ok(Json(serde_json::json!({})))
}
