use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Json, extract::Multipart, extract::Path, extract::Query, extract::State};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, FILE_BLOB, FILES, FileMeta, PageContext, Session, Slug, UserToken};
use book::{CONFIG, crypto::Signed, error::AppError};
use redb::ReadableDatabase;
use serde::Deserialize;
use std::sync::Arc;
use time::{OffsetDateTime, format_description::well_known::Iso8601};

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

// file info / delete
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show file information and management page
pub async fn file_info_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path((category, entry, file)): Path<(String, String, String)>,
) -> Result<Html<String>, AppError> {
    let category = Slug::new(category).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let entry = Slug::new(entry).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let file = Slug::new(file).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let key = (category, entry, file);
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(meta) = files_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {}/{}/{}", key.0, key.1, key.2),
        ));
    };
    let file_meta = meta.value();

    let blobs_table = tx.open_table(FILE_BLOB)?;
    let size = blobs_table
        .get(&key)?
        .map(|blob| blob.value().len())
        .unwrap_or(0);

    let date = OffsetDateTime::from_unix_timestamp(file_meta.last_modified)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let page = PageContext::new()
        .insert("page_title", &key.2)
        .insert("category", &key.0)
        .insert("entry", &key.1)
        .insert("file", &key.2)
        .insert("page_category", &key.0)
        .insert("page_entry", &key.1)
        .insert("page_file", &key.2)
        .insert("page_kind", "file")
        .insert("file_editor", &file_meta.editor)
        .insert("file_date", &date)
        .insert("file_size", &size)
        .insert("user", &user);
    Ok(Html(page.render("file.html")?))
}

/// delete a file and its blob, redirecting back to the entry page
pub async fn file_delete(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Path((category, entry, file)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let category = Slug::new(category).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let entry = Slug::new(entry).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let file = Slug::new(file).map_err(|e| AppError::new(StatusCode::BAD_REQUEST, e))?;
    let key = (category, entry, file);
    let tx = state.db.begin_write()?;

    let mut files_table = tx.open_table(FILES)?;
    files_table.remove(&key)?;
    drop(files_table);

    let mut blobs_table = tx.open_table(FILE_BLOB)?;
    blobs_table.remove(&key)?;
    drop(blobs_table);

    tx.commit()?;

    let base = CONFIG.base_path();
    Ok(Json(
        serde_json::json!({"redirect": format!("{base}/{}/{}/README.md", key.0, key.1)}),
    ))
}
