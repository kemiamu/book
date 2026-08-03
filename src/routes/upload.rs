use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Json, extract::Multipart, extract::Query, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, FILE_BLOB, FILES, FileMeta, PageContext, Session, Slug, UserToken};
use book::{CONFIG, crypto::Signed, error::AppError};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct UploadQuery {
    pub category: Option<String>,
    pub entry: Option<String>,
}

/// show file upload page
pub async fn file_upload_page(
    _token: UserToken,
    jar: CookieJar,
    Query(params): Query<UploadQuery>,
) -> Result<Response, AppError> {
    // uploads are always bound to an entry, so a bare /upload has no target
    let (Some(category), Some(entry)) = (params.category, params.entry) else {
        return Ok(Redirect::to("/").into_response());
    };

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);
    let page = PageContext::new()
        .insert("page_title", "Upload File")
        .insert("user", &user)
        .insert("default_category", &category)
        .insert("default_entry", &entry);
    Ok(Html(page.render("upload.html")?).into_response())
}

/// handle file upload
pub async fn file_upload_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let username = token?;

    let mut category_slug = String::new();
    let mut entry_slug = String::new();
    let mut file_slug = String::new();
    let mut file_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "category_slug" => {
                category_slug = field.text().await.map_err(|e| {
                    AppError::new(
                        StatusCode::BAD_REQUEST,
                        format!("invalid category slug: {e}"),
                    )
                })?
            }
            "entry_slug" => {
                entry_slug = field.text().await.map_err(|e| {
                    AppError::new(StatusCode::BAD_REQUEST, format!("invalid entry slug: {e}"))
                })?
            }
            "file_slug" => {
                file_slug = field.text().await.map_err(|e| {
                    AppError::new(StatusCode::BAD_REQUEST, format!("invalid file slug: {e}"))
                })?
            }
            "file" => {
                if file_data.is_some() {
                    return Err(AppError::new(
                        StatusCode::BAD_REQUEST,
                        "only one file allowed",
                    ));
                }
                let data = field.bytes().await.map_err(|e| {
                    AppError::new(StatusCode::BAD_REQUEST, format!("failed to read file: {e}"))
                })?;
                if data.is_empty() {
                    return Err(AppError::new(StatusCode::BAD_REQUEST, "empty file"));
                }
                if data.len() > 100 * 1024 * 1024 {
                    return Err(AppError::new(
                        StatusCode::BAD_REQUEST,
                        "file too large (max 100 MiB)",
                    ));
                }
                file_data = Some(data.to_vec());
            }
            _ => {}
        }
    }

    if category_slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Category slug must not be empty",
        ));
    }
    if entry_slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Entry slug must not be empty",
        ));
    }
    if file_slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "File slug must not be empty",
        ));
    }
    let Some(data) = file_data else {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "No file uploaded"));
    };

    let category_slug =
        Slug::new(category_slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let entry_slug =
        Slug::new(entry_slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let file_slug = Slug::new(file_slug).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;

    let tx = state.db.begin_write()?;

    let mut files_table = tx.open_table(FILES)?;
    let key = (category_slug, entry_slug, file_slug);
    let meta = FileMeta::new(username);
    files_table.insert(&key, meta)?;
    drop(files_table);

    let mut blobs_table = tx.open_table(FILE_BLOB)?;
    blobs_table.insert(&key, data)?;
    drop(blobs_table);

    tx.commit()?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({}))))
}
