use super::{BreadcrumbItem, HeaderAction, path_slug};
use axum::extract::multipart::Field;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Json, extract::Multipart, extract::Path, extract::Query, extract::State};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, CategoryKey, EntryKey, FILE_BLOB, FILES};
use book::model::{FileKey, FileMeta, PageContext, Session, UserToken};
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

    let mut form = UploadForm::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, format!("multipart error: {e}")))?
    {
        read_field(field, &mut form).await?;
    }
    let UploadForm {
        category_slug,
        entry_slug,
        file_slug,
        file_data,
    } = form;

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

    let category_slug = path_slug::<CategoryKey>(&category_slug)?;
    let entry_slug = path_slug::<EntryKey>(&entry_slug)?;
    let file_slug = path_slug::<FileKey>(&file_slug)?;

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
    let category = path_slug::<CategoryKey>(&category)?;
    let entry = path_slug::<EntryKey>(&entry)?;
    let file = path_slug::<FileKey>(&file)?;
    let key = (category, entry, file);
    let (category, entry, file) = &key;
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(meta) = files_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {category}/{entry}/{file}"),
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

    let base = CONFIG.base_path();
    let breadcrumbs = [
        BreadcrumbItem {
            href: Some(format!("{base}/{category}/README.md")),
            label: key.0.to_string(),
        },
        BreadcrumbItem {
            href: Some(format!("{base}/{category}/{entry}/README.md")),
            label: key.1.to_string(),
        },
        BreadcrumbItem {
            href: None,
            label: key.2.to_string(),
        },
    ];
    let page = PageContext::new()
        .insert("page_title", &key.2)
        .insert("category", &key.0)
        .insert("entry", &key.1)
        .insert("file", &key.2)
        .insert("breadcrumbs", &breadcrumbs)
        .insert("page_actions", &Vec::<HeaderAction>::new())
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
    let category = path_slug::<CategoryKey>(&category)?;
    let entry = path_slug::<EntryKey>(&entry)?;
    let file = path_slug::<FileKey>(&file)?;
    let key = (category, entry, file);
    let (category, entry, _) = &key;
    let tx = state.db.begin_write()?;

    let mut files_table = tx.open_table(FILES)?;
    files_table.remove(&key)?;
    drop(files_table);

    let mut blobs_table = tx.open_table(FILE_BLOB)?;
    blobs_table.remove(&key)?;
    drop(blobs_table);

    tx.commit()?;

    let base = CONFIG.base_path();
    let redirect = serde_json::json!({"redirect": format!("{base}/{category}/{entry}/README.md")});
    Ok(Json(redirect))
}

// upload form parsing
//
// ++++++++++++============++++++++++++============++++++++++++============

/// multipart upload form being accumulated field by field
#[derive(Default)]
struct UploadForm {
    category_slug: String,
    entry_slug: String,
    file_slug: String,
    file_data: Option<Vec<u8>>,
}

/// dispatch one multipart field into the form being built
async fn read_field(field: Field<'_>, form: &mut UploadForm) -> Result<(), AppError> {
    const ONLY_ONE_FILE: &str = "only one file allowed";
    let name = field.name().unwrap_or("").to_string();
    match name.as_str() {
        "category_slug" => form.category_slug = read_text(field, "category slug").await?,
        "entry_slug" => form.entry_slug = read_text(field, "entry slug").await?,
        "file_slug" => form.file_slug = read_text(field, "file slug").await?,
        "file" if form.file_data.is_none() => form.file_data = Some(read_file(field).await?),
        "file" => Err(AppError::new(StatusCode::BAD_REQUEST, ONLY_ONE_FILE))?,
        _ => {}
    }
    Ok(())
}

/// read a text field, tagging errors with the field name
async fn read_text(field: Field<'_>, what: &str) -> Result<String, AppError> {
    field
        .text()
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, format!("invalid {what}: {e}")))
}

/// read the uploaded file, enforcing the size limits
async fn read_file(field: Field<'_>) -> Result<Vec<u8>, AppError> {
    let data = field
        .bytes()
        .await
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, format!("failed to read file: {e}")))?;
    if data.is_empty() {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "empty file"));
    }
    if data.len() > 100 * 1024 * 1024 {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "file too large (max 100 MiB)",
        ));
    }
    Ok(data.to_vec())
}
