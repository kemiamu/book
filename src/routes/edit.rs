use super::{parse_category, parse_entry_id, session_user};
use axum::{Json, extract::Query, extract::State, http::StatusCode, response::Html};
use axum_extra::extract::cookie::CookieJar;
use book::model::{
    AppState, ENTRIES, ENTRY_BODY, EntryMeta, Markdown, PageContext, UserToken, next_entry_id,
};
use book::{CONFIG, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
/// edit entry query params
pub struct EditQuery {
    pub entry_id: Option<String>,
}

/// show edit entry page
pub async fn edit_page(
    jar: CookieJar,
    _token: UserToken,
    State(state): State<Arc<AppState>>,
    Query(params): Query<EditQuery>,
) -> Result<Html<String>, AppError> {
    let (entry_id, category, title, body) = if let Some(raw) = &params.entry_id {
        let entry_id = parse_entry_id(raw)?;
        let tx = state.db.begin_read()?;

        let meta = tx
            .open_table(ENTRIES)?
            .get(&entry_id)?
            .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("entry not found: {raw}")))?
            .value();
        let body = tx
            .open_table(ENTRY_BODY)?
            .get(&entry_id)?
            .ok_or_else(|| {
                AppError::new(
                    StatusCode::NOT_FOUND,
                    format!("entry body not found: {raw}"),
                )
            })?
            .value()
            .raw()
            .to_string();

        (raw.clone(), meta.category, meta.title, body)
    } else {
        (String::new(), String::new(), String::new(), String::new())
    };

    let page = PageContext::new()
        .insert("page_title", "Edit")
        .insert("entry_id", &entry_id)
        .insert("category", &category)
        .insert("title", &title)
        .insert("body", &body)
        .insert("error", "")
        .insert("user", &session_user(&jar));
    Ok(Html(page.render("edit.html")?))
}

#[derive(Deserialize)]
/// edit form payload
pub struct EditForm {
    pub entry_id: Option<String>,
    pub category: String,
    pub title: String,
    pub body: String,
}

/// handle page save, creating a new entry or updating an existing one
pub async fn edit_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Json(body): Json<EditForm>,
) -> Result<Json<serde_json::Value>, AppError> {
    let username = token?;
    let category = parse_category(&body.category)?;
    if body.title.trim().is_empty() {
        return Err(AppError::json(
            StatusCode::BAD_REQUEST,
            "title must not be empty",
        ));
    }

    let mut tx = state.db.begin_write()?;
    let entry_id = match body.entry_id {
        Some(raw) => {
            let entry_id = parse_entry_id(&raw)?;
            let meta = tx
                .open_table(ENTRIES)?
                .get(&entry_id)?
                .ok_or_else(|| AppError::json(StatusCode::NOT_FOUND, "entry not found"))?
                .value()
                .update(body.title.as_str(), category.as_str(), username);
            tx.open_table(ENTRIES)?.insert(&entry_id, meta)?;
            entry_id
        }
        None => {
            let entry_id = next_entry_id(&mut tx)?;
            let meta = EntryMeta::new(body.title.as_str(), category.as_str(), username);
            tx.open_table(ENTRIES)?.insert(&entry_id, meta)?;
            entry_id
        }
    };

    let md = Markdown::new(body.body);
    tx.open_table(ENTRY_BODY)?.insert(&entry_id, md)?;
    tx.commit()?;

    let base = CONFIG.base_path();
    Ok(Json(
        serde_json::json!({"redirect": format!("{base}/{entry_id}/README.md")}),
    ))
}
