use axum::Json;
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum_extra::extract::cookie::CookieJar;
use book::CONFIG;
use book::crypto::Signed;
use book::model::res::{FILE_BLOBS, FILES, FileBlob, ResourceMeta};
use book::model::user::{Session, UserToken};
use book::model::{AppState, PageContext, error::AppError};
use redb::ReadableTable;
use std::collections::HashSet;
use std::sync::Arc;

/// show file upload page
pub async fn file_upload_page(_token: UserToken, jar: CookieJar) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);
    let page = PageContext::new()
        .insert("page_title", "Upload File")
        .insert("user", &user);
    Ok(Html(page.render("upload.tera")?))
}

/// handle file upload
pub async fn file_upload_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), AppError> {
    let username = token?;

    let mut slug = String::new();
    let mut title = String::new();
    let mut file_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "slug" => {
                slug = field.text().await.map_err(|e| {
                    AppError::new(StatusCode::BAD_REQUEST, format!("invalid slug: {e}"))
                })?
            }
            "title" => {
                title = field.text().await.map_err(|e| {
                    AppError::new(StatusCode::BAD_REQUEST, format!("invalid title: {e}"))
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

    if slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Slug must not be empty",
        ));
    }
    if title.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Title must not be empty",
        ));
    }
    let Some(data) = file_data else {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "No file uploaded"));
    };

    let tx = state.db.begin_write()?;

    let mut files_table = tx.open_table(FILES)?;
    if files_table.get(slug.as_str())?.is_some() {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            format!("A file with slug '{slug}' already exists"),
        ));
    }

    let meta = ResourceMeta::new(&title, &username, HashSet::new());
    files_table.insert(slug.as_str(), meta)?;
    drop(files_table);

    let mut blobs_table = tx.open_table(FILE_BLOBS)?;
    blobs_table.insert(slug.as_str(), FileBlob(data))?;
    drop(blobs_table);

    tx.commit()?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({}))))
}
