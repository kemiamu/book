use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::model::res::ResourceMeta;
use book::model::user::{Session, UserToken};
use book::model::{AppState, PageContext, error::AppError};
use book::model::{PAGE_HTML, PAGE_RAW, PAGES};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;

use std::sync::Arc;

#[derive(Deserialize)]
/// edit page query params
pub struct EditQuery {
    pub page: Option<String>,
}

/// show edit page
pub async fn edit_page(
    jar: CookieJar,
    _token: UserToken,
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditQuery>,
) -> Result<Html<String>, AppError> {
    let (slug, title, body) = if let Some(ref page_slug) = params.page {
        let tx = state.db.begin_read()?;

        let pages_table = tx.open_table(PAGES)?;
        let meta = pages_table.get(page_slug.as_str())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("page not found: {page_slug}"),
            )
        })?;

        let bodies_table = tx.open_table(PAGE_RAW)?;
        let body = bodies_table.get(page_slug.as_str())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("page body not found: {page_slug}"),
            )
        })?;

        (
            page_slug.clone(),
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

    let mut pages_table = tx.open_table(PAGES)?;
    let existing = pages_table.get(body.slug.as_str())?.map(|g| g.value());
    let meta = ResourceMeta::new(
        &body.title,
        &username,
        existing.map(|m| m.tags.clone()).unwrap_or_default(),
    );
    pages_table.insert(body.slug.as_str(), meta)?;
    drop(pages_table);

    let md = book::model::res::Markdown::new(body.body.clone());
    let html = md.render();

    let mut raw_table = tx.open_table(PAGE_RAW)?;
    raw_table.insert(body.slug.as_str(), md)?;
    drop(raw_table);

    let mut html_table = tx.open_table(PAGE_HTML)?;
    html_table.insert(body.slug.as_str(), html)?;
    drop(html_table);

    tx.commit()?;

    Ok(Json(serde_json::json!({})))
}
