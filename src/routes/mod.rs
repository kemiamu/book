use axum::response::{Html, IntoResponse};
use axum::{Json, extract::Path, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use book::model::{
    AppState, CategoryKey, ENTRIES, ENTRY_BODY, EntryKey, EntryMeta, FileKey, SlugRule,
};
use book::model::{FILE_BLOB, FILES, FilePath, PageContext, Passkey, Session, Slug, UserToken};
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

// delete
//
// ++++++++++++============++++++++++++============++++++++++++============

/// delete an entry and all its files
pub async fn entry_delete(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Path((category, entry)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _username = token?;
    let category = path_slug::<CategoryKey>(&category)?;
    let entry = path_slug::<EntryKey>(&entry)?;
    let tx = state.db.begin_write()?;

    // collect all file keys for this entry
    let files_to_remove: Vec<FilePath> = {
        let files_table = tx.open_table(FILES)?;
        let mut keys = Vec::new();
        for result in files_table.iter()? {
            let (key, _) = result?;
            let (file_category, file_entry, file_name) = key.value();
            if file_category == category && file_entry == entry {
                keys.push((file_category, file_entry, file_name));
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
    let entry_key = (category, entry);
    {
        let mut entries_table = tx.open_table(ENTRIES)?;
        entries_table.remove(&entry_key)?;
    }
    {
        let mut body_table = tx.open_table(ENTRY_BODY)?;
        body_table.remove(&entry_key)?;
    }

    tx.commit()?;

    Ok(Json(
        serde_json::json!({"redirect": format!("{}/", CONFIG.base_path())}),
    ))
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

/// parse a path segment into a slug, normalizing disallowed characters
fn path_slug<T: SlugRule>(raw: &str) -> Result<Slug<T>, AppError> {
    Slug::normalize(raw).ok_or_else(|| AppError::new(StatusCode::BAD_REQUEST, "invalid slug"))
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
    let mut entries: Vec<(Slug<CategoryKey>, Slug<EntryKey>, EntryMeta)> = Vec::new();
    let mut categories: BTreeSet<Slug<CategoryKey>> = BTreeSet::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let (category, entry) = key.value();
        entries.push((category.clone(), entry, value.value()));
        categories.insert(category);
    }

    // most recently updated first
    entries.sort_by(|a, b| b.2.last_modified.cmp(&a.2.last_modified));

    let base = CONFIG.base_path();
    let entries: Vec<ListItem> = entries
        .into_iter()
        .map(|(category, entry, meta)| ListItem {
            href: format!("{base}/{category}/{entry}/README.md"),
            title: meta.title,
        })
        .collect();

    let categories: Vec<ListItem> = categories
        .into_iter()
        .map(|category| ListItem {
            href: format!("{base}/{category}/README.md"),
            title: category.to_string(),
        })
        .collect();

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("categories", &categories)
        .insert("entries", &entries)
        .insert("user", &user);
    Ok(Html(page.render("home.html")?))
}

// categories
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show entries filtered by a category
pub async fn category_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(category): Path<String>,
) -> Result<Html<String>, AppError> {
    let category = path_slug::<CategoryKey>(&category)?;
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let mut entries: Vec<(Slug<CategoryKey>, Slug<EntryKey>, EntryMeta)> = Vec::new();
    for result in entries_table.iter()? {
        let (key, value) = result?;
        let (entry_category, entry) = key.value();
        if entry_category == category {
            entries.push((entry_category, entry, value.value()));
        }
    }

    // most recently updated first
    entries.sort_by(|a, b| b.2.last_modified.cmp(&a.2.last_modified));

    let base = CONFIG.base_path();
    let entries: Vec<ListItem> = entries
        .into_iter()
        .map(|(category, entry, meta)| ListItem {
            href: format!("{base}/{category}/{entry}/README.md"),
            title: meta.title,
        })
        .collect();

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let page = PageContext::new()
        .insert("page_title", &format!("Category: {category}"))
        .insert("category", &category)
        .insert(
            "breadcrumbs",
            &[BreadcrumbItem {
                href: None,
                label: category.to_string(),
            }],
        )
        .insert("page_actions", &Vec::<HeaderAction>::new())
        .insert("entries", &entries)
        .insert("user", &user);
    Ok(Html(page.render("category.html")?))
}

// view
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show an entry page
pub async fn entry_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path((category, entry)): Path<(String, String)>,
) -> Result<Html<String>, AppError> {
    let category = path_slug::<CategoryKey>(&category)?;
    let entry = path_slug::<EntryKey>(&entry)?;
    let key = (category, entry);
    let (category, entry) = &key;
    let tx = state.db.begin_read()?;

    let entries_table = tx.open_table(ENTRIES)?;
    let Some(row) = entries_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry not found: {category}/{entry}"),
        ));
    };

    let body_table = tx.open_table(ENTRY_BODY)?;
    let Some(body) = body_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("entry body not found: {category}/{entry}"),
        ));
    };

    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let entry_meta = row.value();
    let editor = &entry_meta.editor;
    let date = OffsetDateTime::from_unix_timestamp(entry_meta.last_modified)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let files_table = tx.open_table(FILES)?;
    let mut files: Vec<Slug<FileKey>> = Vec::new();
    for result in files_table.iter()? {
        let (file_key, _) = result?;
        let (file_category, file_entry, file_name) = file_key.value();
        if file_category == key.0 && file_entry == key.1 {
            files.push(file_name);
        }
    }

    let base = CONFIG.base_path();
    let files: Vec<ListItem> = files
        .into_iter()
        .map(|file| ListItem {
            href: format!("{base}/{category}/{entry}/{file}/info"),
            title: file.to_string(),
        })
        .collect();

    let page = PageContext::new()
        .insert("page_title", &entry_meta.title)
        .insert("content", &body.value().html)
        .insert("user", &user)
        .insert("category", &key.0)
        .insert("entry", &key.1)
        .insert(
            "breadcrumbs",
            &[
                BreadcrumbItem {
                    href: Some(format!("{base}/{category}/README.md")),
                    label: key.0.to_string(),
                },
                BreadcrumbItem {
                    href: None,
                    label: format!("{date} @ {editor}"),
                },
            ],
        )
        .insert(
            "page_actions",
            &[
                HeaderAction {
                    href: format!("{base}/edit?category={category}&entry={entry}"),
                    label: "Edit".into(),
                },
                HeaderAction {
                    href: format!("{base}/upload?category={category}&entry={entry}"),
                    label: "Upload".into(),
                },
            ],
        )
        .insert("page_date", &date)
        .insert("page_editor", &entry_meta.editor)
        .insert("files", &files);
    Ok(Html(page.render("entry.html")?))
}

// profile
//
// ++++++++++++============++++++++++++============++++++++++++============

/// show profile page
pub async fn profile_page(
    jar: CookieJar,
    UserToken(token): UserToken,
) -> Result<Html<String>, AppError> {
    let user = jar
        .get("session")
        .and_then(|cookie| Signed::<Session>::parse(cookie.value(), &CONFIG.secret))
        .map(|session| session.inner.user);

    let passkey = Passkey::new(token?);
    let expires_at = passkey.expires_at();
    let signed = Signed::new(passkey);
    let code = signed.generate(&CONFIG.secret);

    let expires_at = OffsetDateTime::from_unix_timestamp(expires_at)
        .ok()
        .and_then(|date| date.format(&Iso8601::DATE).ok())
        .unwrap_or_default();

    let base_url = &CONFIG.base_url;
    let passkey_url = format!("{base_url}/auth?passkey={code}");

    let page = PageContext::new()
        .insert("page_title", "Profile")
        .insert("user", &user)
        .insert("passkey_url", &passkey_url)
        .insert("passkey_code_expiry", &expires_at);
    Ok(Html(page.render("profile.html")?))
}

// download
//
// ++++++++++++============++++++++++++============++++++++++++============

/// download a file by category, entry and file slug
pub async fn file_download(
    State(state): State<Arc<AppState>>,
    Path((category, entry, file)): Path<(String, String, String)>,
) -> Result<(StatusCode, impl IntoResponse), AppError> {
    let category = path_slug::<CategoryKey>(&category)?;
    let entry = path_slug::<EntryKey>(&entry)?;
    let file = path_slug::<FileKey>(&file)?;
    let key = (category, entry, file);
    let (category, entry, file) = &key;
    let tx = state.db.begin_read()?;

    let files_table = tx.open_table(FILES)?;
    let Some(_meta) = files_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file not found: {category}/{entry}/{file}"),
        ));
    };
    drop(files_table);

    let blobs_table = tx.open_table(FILE_BLOB)?;
    let Some(blob) = blobs_table.get(&key)? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("file blob not found: {category}/{entry}/{file}"),
        ));
    };
    let data = blob.value();
    drop(blobs_table);

    let content_type =
        mime_guess::from_path(key.2.as_ref()).first_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let headers = [
        ("Content-Type", content_type.to_string()),
        (
            "Content-Disposition",
            format!("inline; filename=\"{file}\""),
        ),
    ];

    Ok((StatusCode::OK, (headers, data)))
}

// robots
//
// ++++++++++++============++++++++++++============++++++++++++============

/// serve robots.txt honoring the base path, so only the home page is crawlable
pub async fn robots_txt() -> impl IntoResponse {
    let base = CONFIG.base_path();
    let body = format!("User-agent: *\nAllow: {base}/$\nDisallow: /\n");
    ([(axum::http::header::CONTENT_TYPE, "text/plain")], body)
}
