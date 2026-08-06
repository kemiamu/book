use super::{
    BreadcrumbItem, HeaderAction, display_category, format_date, parse_entry_id, parse_file_slug,
    session_user,
};
use axum::extract::multipart::Field;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Json, extract::Multipart, extract::Path, extract::Query, extract::State};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, ENTRIES, FILE_BLOB, FILES, FileMeta, PageContext, UserToken};
use book::{CONFIG, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct UploadQuery {
    pub entry_id: Option<String>,
}

/// show file upload page
pub async fn file_upload_page(
    jar: CookieJar,
    _token: UserToken,
    State(state): State<Arc<AppState>>,
    Query(params): Query<UploadQuery>,
) -> Result<Response, AppError> {
    // uploads are always bound to an entry, so a bare /upload has no target
    let Some(raw) = params.entry_id else {
        return Ok(Redirect::to(CONFIG.base_path()).into_response());
    };
    let entry_id = parse_entry_id(&raw)?;
    let tx = state.db.begin_read()?;
    let meta = tx
        .open_table(ENTRIES)?
        .get(&entry_id)?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, format!("entry not found: {raw}")))?
        .value();

    let page = PageContext::new()
        .insert("page_title", "Upload File")
        .insert("user", &session_user(&jar))
        .insert("entry_id", &entry_id)
        .insert("category", &display_category(&meta.category))
        .insert("entry_title", &meta.title);
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
        entry_id: raw_entry_id,
        file_slug: raw_file_slug,
        file_data,
    } = form;

    if raw_entry_id.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "Entry id must not be empty",
        ));
    }
    if raw_file_slug.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "File slug must not be empty",
        ));
    }
    let Some(data) = file_data else {
        return Err(AppError::new(StatusCode::BAD_REQUEST, "No file uploaded"));
    };

    let entry_id = parse_entry_id(&raw_entry_id)?;
    let file_name = parse_file_slug(&raw_file_slug)?;
    let key = (entry_id, file_name);

    let tx = state.db.begin_write()?;
    let entries_table = tx.open_table(ENTRIES)?;
    if entries_table.get(&key.0)?.is_none() {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry not found: {raw_entry_id}"),
        ));
    }
    drop(entries_table);

    let meta = FileMeta::new(username);
    tx.open_table(FILES)?.insert(&key, meta)?;
    tx.open_table(FILE_BLOB)?.insert(&key, data.as_slice())?;
    tx.commit()?;

    let base = CONFIG.base_path();
    let redirect = serde_json::json!({"redirect": format!("{base}/{}/README.md", key.0)});
    Ok((StatusCode::CREATED, Json(redirect)))
}

// file info / delete
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show file information and management page
pub async fn file_info_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path((raw_entry_id, raw_file)): Path<(String, String)>,
) -> Result<Html<String>, AppError> {
    let entry_id = parse_entry_id(&raw_entry_id)?;
    let file = parse_file_slug(&raw_file)?;
    let key = (entry_id, file.clone());
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(meta) = files_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {raw_entry_id}/{raw_file}"),
        ));
    };
    let file_meta = meta.value();

    let entry_meta = tx
        .open_table(ENTRIES)?
        .get(&key.0)?
        .ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry not found: {raw_entry_id}"),
            )
        })?
        .value();

    let size = tx
        .open_table(FILE_BLOB)?
        .get(&key)?
        .map(|blob| blob.value().len())
        .unwrap_or(0);
    let date = format_date(file_meta.last_modified);
    let user = session_user(&jar);

    let base = CONFIG.base_path();
    let breadcrumbs = [
        BreadcrumbItem {
            href: Some(format!("{base}/{raw_entry_id}/README.md")),
            label: entry_meta.title.clone(),
        },
        BreadcrumbItem {
            href: None,
            label: file.to_string(),
        },
    ];
    let page = PageContext::new()
        .insert("page_title", &file)
        .insert("entry_id", &raw_entry_id)
        .insert("file", &file)
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
    Path((raw_entry_id, raw_file)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let entry_id = parse_entry_id(&raw_entry_id)?;
    let file_name = parse_file_slug(&raw_file)?;
    let key = (entry_id, file_name);
    let tx = state.db.begin_write()?;

    tx.open_table(FILES)?.remove(&key)?;
    tx.open_table(FILE_BLOB)?.remove(&key)?;
    tx.commit()?;

    let base = CONFIG.base_path();
    let redirect = serde_json::json!({"redirect": format!("{base}/{raw_entry_id}/README.md")});
    Ok(Json(redirect))
}

// upload form parsing
//
// ++++++++++++============++++++++++++============++++++++++++============

/// multipart upload form being accumulated field by field
#[derive(Default)]
struct UploadForm {
    entry_id: String,
    file_slug: String,
    file_data: Option<Vec<u8>>,
}

/// dispatch one multipart field into the form being built
async fn read_field(field: Field<'_>, form: &mut UploadForm) -> Result<(), AppError> {
    const ONLY_ONE_FILE: &str = "only one file allowed";
    let name = field.name().unwrap_or("").to_string();
    match name.as_str() {
        "entry_id" => form.entry_id = read_text(field, "entry id").await?,
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
