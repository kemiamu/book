use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::model::res::{Markdown, PAGE_BODIES, PAGES, ResourceMeta};
use book::model::user::{Session, UserToken};
use book::model::{AppState, PageContext, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::collections::HashSet;
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

        let bodies_table = tx.open_table(PAGE_BODIES)?;
        let body = bodies_table.get(page_slug.as_str())?.ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("page body not found: {page_slug}"),
            )
        })?;

        (
            page_slug.clone(),
            meta.value().title.clone(),
            body.value().0.clone(),
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
    let existing = pages_table
        .get(body.slug.as_str())?
        .map(|g| g.value().clone());
    let meta = match existing {
        Some(existing_meta) => ResourceMeta::new(
            &body.title,
            &existing_meta.creator,
            existing_meta.tags.clone(),
        ),
        None => ResourceMeta::new(&body.title, &username, HashSet::new()),
    };
    pages_table.insert(body.slug.as_str(), meta)?;
    drop(pages_table);

    let mut bodies_table = tx.open_table(PAGE_BODIES)?;
    bodies_table.insert(body.slug.as_str(), Markdown(body.body.clone()))?;
    drop(bodies_table);

    tx.commit()?;

    Ok(Json(serde_json::json!({})))
}
