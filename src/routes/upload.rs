use axum::Json;
use axum::extract::{Multipart, Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::error::AppError;
use book::model::FileMeta;
use book::model::{AppState, PageContext, Session, UserToken};
use book::model::{FILE_BLOB, FILES};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct UploadQuery {
    pub entry: Option<String>,
}

/// show file upload page
pub async fn file_upload_page(
    _token: UserToken,
    jar: CookieJar,
    Query(params): Query<UploadQuery>,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);
    let page = PageContext::new()
        .insert("page_title", "Upload File")
        .insert("user", &user)
        .insert("default_entry", &params.entry);
    Ok(Html(page.render("upload.html")?))
}

/// handle file upload
pub async fn file_upload_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let username = token?;

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

    let tx = state.db.begin_write()?;

    let mut files_table = tx.open_table(FILES)?;
    let key = (entry_slug.as_str(), file_slug.as_str());
    let meta = FileMeta::new(&username);
    files_table.insert(key, meta)?;
    drop(files_table);

    let mut blobs_table = tx.open_table(FILE_BLOB)?;
    blobs_table.insert(key, data)?;
    drop(blobs_table);

    tx.commit()?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({}))))
}
