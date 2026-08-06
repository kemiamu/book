use axum::response::{Html, IntoResponse};
use axum::{Json, extract::Path, extract::Query, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use book::model::{AppState, ENTRIES, ENTRY_BODY, EntryId, EntryMeta, Passkey};
use book::model::{FILE_BLOB, FILES, FileName, PageContext, Session, Slug, UserName, UserToken};
use book::{CONFIG, crypto::Signed, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use std::{collections::BTreeSet, sync::Arc};
use time::{OffsetDateTime, format_description::well_known::Iso8601};

mod auth;
mod edit;
mod upload;

pub use auth::*;
pub use edit::*;
pub use upload::*;

// home
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show home page with entries and categories
pub async fn home_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries: Vec<(EntryId, EntryMeta)> = Vec::new();
    let mut categories: BTreeSet<String> = BTreeSet::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let meta = value.value();
        categories.insert(meta.category.clone());
        entries.push((key.value(), meta));
    }

    // most recently updated first
    entries.sort_by(|a, b| b.1.last_modified.cmp(&a.1.last_modified));

    let base = CONFIG.base_path();
    let entries: Vec<ListItem> = entries
        .into_iter()
        .map(|(id, meta)| ListItem {
            href: format!("{base}/{id}/README.md"),
            title: meta.title,
        })
        .collect();
    let categories: Vec<ListItem> = categories
        .into_iter()
        .map(|category| ListItem {
            href: format!("{base}/category?name={}", encode_query(&category)),
            title: category,
        })
        .collect();

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("categories", &categories)
        .insert("entries", &entries)
        .insert("user", &session_user(&jar));
    Ok(Html(page.render("home.html")?))
}

// view
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show an entry page for a hex entry id
pub async fn entry_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(raw_entry_id): Path<String>,
) -> Result<Html<String>, AppError> {
    let entry_id = parse_entry_id(&raw_entry_id)?;
    let tx = state.db.begin_read()?;

    let meta = tx
        .open_table(ENTRIES)?
        .get(&entry_id)?
        .ok_or_else(|| {
            AppError::new(
                StatusCode::NOT_FOUND,
                format!("entry not found: {raw_entry_id}"),
            )
        })?
        .value();
    render_entry_page(jar, &entry_id, meta, &tx).await
}

/// render the entry page for a found entry
async fn render_entry_page(
    jar: CookieJar,
    entry_id: &EntryId,
    meta: EntryMeta,
    tx: &redb::ReadTransaction,
) -> Result<Html<String>, AppError> {
    let body_table = tx.open_table(ENTRY_BODY)?;
    let Some(body) = body_table.get(entry_id)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry body not found: {entry_id}"),
        ));
    };

    let files_table = tx.open_table(FILES)?;
    let mut files: Vec<Slug<FileName>> = Vec::new();
    for result in files_table.iter()? {
        let (key, _) = result?;
        let (file_entry_id, file_name) = key.value();
        if &file_entry_id == entry_id {
            files.push(file_name);
        }
    }

    let base = CONFIG.base_path();
    let files: Vec<ListItem> = files
        .into_iter()
        .map(|file| ListItem {
            href: format!("{base}/{entry_id}/info/{file}"),
            title: file.to_string(),
        })
        .collect();

    // the breadcrumb shows when the entry was created
    let date = format_date(meta.created_at);
    let breadcrumbs = [BreadcrumbItem {
        href: None,
        label: format!("{date} @ {}", meta.editor),
    }];
    let page_actions = [
        HeaderAction {
            href: format!("{base}/edit?entry_id={entry_id}"),
            label: "Edit".into(),
        },
        HeaderAction {
            href: format!("{base}/upload?entry_id={entry_id}"),
            label: "Upload".into(),
        },
    ];
    let page = PageContext::new()
        .insert("page_title", &meta.title)
        .insert("content", body.value().html())
        .insert("user", &session_user(&jar))
        .insert("entry_id", entry_id)
        .insert("breadcrumbs", &breadcrumbs)
        .insert("page_actions", &page_actions)
        .insert("files", &files);
    Ok(Html(page.render("entry.html")?))
}

// categories
//
// ++++++++++++============++++++++++++============++++++++++++============

/// category page query: the category name carried as a query parameter
#[derive(serde::Deserialize)]
pub struct CategoryQuery {
    name: Option<String>,
}

/// show the category page: entries whose category matches the query name
pub async fn category_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Query(params): Query<CategoryQuery>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;
    let name = params.name.unwrap_or_default();
    render_category_page(jar, &name, &tx).await
}

/// render the category page: entries whose category matches the name
async fn render_category_page(
    jar: CookieJar,
    category: &str,
    tx: &redb::ReadTransaction,
) -> Result<Html<String>, AppError> {
    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries: Vec<(EntryId, EntryMeta)> = Vec::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let meta = value.value();
        if meta.category == category {
            entries.push((key.value(), meta));
        }
    }
    entries.sort_by(|a, b| b.1.last_modified.cmp(&a.1.last_modified));

    let base = CONFIG.base_path();
    let entries: Vec<ListItem> = entries
        .into_iter()
        .map(|(id, meta)| ListItem {
            href: format!("{base}/{id}/README.md"),
            title: meta.title,
        })
        .collect();

    let breadcrumbs = [BreadcrumbItem {
        href: None,
        label: category.to_string(),
    }];
    let page = PageContext::new()
        .insert("page_title", &format!("Category: {category}"))
        .insert("category", &category)
        .insert("breadcrumbs", &breadcrumbs)
        .insert("page_actions", &Vec::<HeaderAction>::new())
        .insert("entries", &entries)
        .insert("user", &session_user(&jar));
    Ok(Html(page.render("category.html")?))
}

// profile
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show the profile page with a fresh invitation passkey
pub async fn profile_page(UserToken(token): UserToken) -> Result<Html<String>, AppError> {
    let username = token?;
    let user = Some(username.clone());
    let passkey = Signed::new(Passkey::new(username)).generate(&CONFIG.secret);
    let expiry = format_date(time::UtcDateTime::now().unix_timestamp() + Passkey::EXPIRY_SECS);
    let base = CONFIG.base_url.as_str();
    let passkey_url = format!("{base}/auth?passkey={passkey}");
    let page = PageContext::new()
        .insert("page_title", "Profile")
        .insert("passkey_url", &passkey_url)
        .insert("passkey_code_expiry", &expiry)
        .insert("user", &user);
    Ok(Html(page.render("profile.html")?))
}

// delete
//
// ++++++++++++============++++++++++++============++++++++++++============

/// delete an entry and all its files
pub async fn entry_delete(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Path(raw_entry_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let entry_id = parse_entry_id(&raw_entry_id)?;
    let tx = state.db.begin_write()?;

    // collect all file keys for this entry
    let files_to_remove: Vec<(EntryId, Slug<FileName>)> = {
        let files_table = tx.open_table(FILES)?;
        let mut keys = Vec::new();
        for result in files_table.iter()? {
            let (key, _) = result?;
            let (file_entry_id, file_name) = key.value();
            if file_entry_id == entry_id {
                keys.push((file_entry_id, file_name));
            }
        }
        keys
    };

    // remove file blobs
    {
        let mut blobs_table = tx.open_table(FILE_BLOB)?;
        for key in &files_to_remove {
            blobs_table.remove(key)?;
        }
    }

    // remove file metadata
    {
        let mut files_table = tx.open_table(FILES)?;
        for key in &files_to_remove {
            files_table.remove(key)?;
        }
    }

    // remove entry data from all tables
    {
        let mut entries_table = tx.open_table(ENTRIES)?;
        entries_table.remove(&entry_id)?;
    }
    {
        let mut body_table = tx.open_table(ENTRY_BODY)?;
        body_table.remove(&entry_id)?;
    }

    tx.commit()?;

    let redirect = serde_json::json!({"redirect": format!("{}/", CONFIG.base_path())});
    Ok(Json(redirect))
}

// download
//
// ++++++++++++============++++++++++++============++++++++++++============

/// download a file, copying the stored bytes into the response
pub async fn file_download(
    State(state): State<Arc<AppState>>,
    Path((raw_entry_id, raw_file)): Path<(String, String)>,
) -> Result<(StatusCode, impl IntoResponse), AppError> {
    let entry_id = parse_entry_id(&raw_entry_id)?;
    let file = parse_file_slug(&raw_file)?;
    let content_type =
        mime_guess::from_path(file.as_ref()).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);
    let disposition = format!("inline; filename=\"{file}\"");
    let key = (entry_id, file);
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    if files_table.get(&key)?.is_none() {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {raw_entry_id}/{raw_file}"),
        ));
    }
    drop(files_table);

    // the read transaction drops when the handler returns, so the
    // borrowed page bytes are copied into an owned response body
    let blobs_table = tx.open_table(FILE_BLOB)?;
    let Some(blob) = blobs_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file blob not found: {raw_entry_id}/{raw_file}"),
        ));
    };
    let data = axum::body::Bytes::copy_from_slice(blob.value());
    drop(blobs_table);

    let headers = [
        ("Content-Type", content_type.to_string()),
        ("Content-Disposition", disposition),
    ];

    Ok((StatusCode::OK, (headers, data)))
}

// util
//
// ++++++++++++============++++++++++++============++++++++++++============

/// view model for a single listing row
#[derive(serde::Serialize)]
struct ListItem {
    href: String,
    title: String,
}

/// view model for a breadcrumb item; the current page has no href
#[derive(serde::Serialize)]
struct BreadcrumbItem {
    href: Option<String>,
    label: String,
}

/// view model for an optional action button in the page header
#[derive(serde::Serialize)]
struct HeaderAction {
    href: String,
    label: String,
}

/// the signed-in user, if any
fn session_user(jar: &CookieJar) -> Option<Slug<UserName>> {
    jar.get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user)
}

/// format a unix timestamp as a date string
fn format_date(timestamp: i64) -> String {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
        .unwrap_or_default()
}

/// parse a path segment as a hex entry id
fn parse_entry_id(raw: &str) -> Result<EntryId, AppError> {
    u64::from_str_radix(raw, 16)
        .map(EntryId::from)
        .map_err(|_| AppError::new(StatusCode::BAD_REQUEST, "invalid entry id"))
}

/// parse a path segment as a file name slug
fn parse_file_slug(raw: &str) -> Result<Slug<FileName>, AppError> {
    Slug::normalize(raw).ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "invalid file name"))
}

/// validate a category: non-empty free text within a sane length
fn parse_category(raw: &str) -> Result<String, AppError> {
    let category = raw.trim();
    if category.is_empty() {
        return Err(AppError::json(
            StatusCode::BAD_REQUEST,
            "category must not be empty",
        ));
    }
    if category.len() > 255 {
        return Err(AppError::json(
            StatusCode::BAD_REQUEST,
            "category too long (max 255 bytes)",
        ));
    }
    Ok(category.to_string())
}

/// percent-encode a category for use as a query parameter value;
/// '/' is kept since query strings are not path-structured
fn encode_query(category: &str) -> String {
    let mut out = String::with_capacity(category.len());
    for b in category.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
