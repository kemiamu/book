use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use book::CONFIG;
use book::crypto::Signed;
use book::model::res::{FILES, Markdown, PAGE_BODIES, PAGES, ResourceMeta};
use book::model::user::{Invitation, Session, USERS, User, UserToken};
use book::model::{AppState, PageContext, error::AppError};
use redb::{ReadableDatabase, ReadableTable};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn internal_error(e: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
    )
}

fn err(status: StatusCode, msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({"error": msg.to_string()})))
}

// home

pub async fn home_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let mut pages: Vec<serde_json::Value> = Vec::new();
    for result in pages_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        pages.push(serde_json::json!({"name": key.value(), "title": r.title}));
    }

    let files_table = tx.open_table(FILES)?;
    let mut files: Vec<serde_json::Value> = Vec::new();
    for result in files_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        files.push(serde_json::json!({"name": key.value(), "title": r.title}));
    }

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let page = PageContext::new()
        .insert("page_title", "Home")
        .insert("pages", &pages)
        .insert("files", &files)
        .insert("user", &user);
    Ok(Html(page.render("home.html")?))
}

// view

pub async fn view_page(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Html<String>, AppError> {
    let tx = state.db.begin_read()?;

    let pages_table = tx.open_table(PAGES)?;
    let Some(meta) = pages_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("page not found: {slug}"),
        ));
    };

    let bodies_table = tx.open_table(PAGE_BODIES)?;
    let Some(body) = bodies_table.get(slug.as_str())? else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            format!("page body not found: {slug}"),
        ));
    };

    let user = jar
        .get("session")
        .and_then(|c| Signed::<Session>::parse(c.value(), &CONFIG.secret))
        .map(|s| s.inner.user);

    let page = PageContext::new()
        .insert("page_title", &meta.value().title)
        .insert("content", &body.value().render())
        .insert("user", &user)
        .insert("slug", &slug);
    Ok(Html(page.render("view.html")?))
}

// sign in

pub async fn sign_in_page() -> Result<Html<String>, AppError> {
    let page = PageContext::new().insert("page_title", "Sign In");
    Ok(Html(page.render("sign-in.html")?))
}

#[derive(Deserialize)]
pub struct SignInForm {
    username: String,
    password: String,
}

pub async fn sign_in_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignInForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let tx = state.db.begin_read().map_err(internal_error)?;
    let table = tx.open_table(USERS).map_err(internal_error)?;

    let user = table
        .get(body.username.as_str())
        .map_err(internal_error)?
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid username or password"))?
        .value();

    if !user.verify(&body.password, &CONFIG.secret) {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign up

pub async fn sign_up_page(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Html<String>, AppError> {
    let invite = params.get("invite").cloned();

    let page = PageContext::new()
        .insert("page_title", "Sign Up")
        .insert("invite", &invite);
    Ok(Html(page.render("sign-up.html")?))
}

#[derive(Deserialize)]
pub struct SignUpForm {
    username: String,
    password: String,
    invite: String,
}

pub async fn sign_up_post(
    jar: CookieJar,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SignUpForm>,
) -> Result<(CookieJar, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let invitation = Signed::<Invitation>::parse(&body.invite, &CONFIG.secret)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "Invalid or expired invite code"))?;

    let tx = state.db.begin_write().map_err(internal_error)?;
    let mut table = tx.open_table(USERS).map_err(internal_error)?;

    if table
        .get(body.username.as_str())
        .map_err(internal_error)?
        .is_some()
    {
        return Err(err(StatusCode::CONFLICT, "Username already exists"));
    }

    let user = User::new(&body.password, &CONFIG.secret, invitation.inner.inviter);
    table
        .insert(body.username.as_str(), user)
        .map_err(internal_error)?;
    drop(table);
    tx.commit().map_err(internal_error)?;

    let jar = set_session_cookie(jar, &body.username, &CONFIG.secret);
    Ok((jar, Json(serde_json::json!({}))))
}

// sign out

pub async fn sign_out(jar: CookieJar) -> impl IntoResponse {
    let jar = jar.remove(Cookie::from("session"));
    (jar, Redirect::to("/"))
}

// profile

pub async fn profile_page(UserToken(_token): UserToken) -> Result<Html<String>, AppError> {
    let page = PageContext::new().insert("page_title", "Profile");
    Ok(Html(page.render("profile.html")?))
}

pub async fn generate_invite(
    UserToken(token): UserToken,
) -> Result<Json<serde_json::Value>, AppError> {
    let invitation = Signed::new(Invitation::new(&token?));
    let code = invitation.generate(&CONFIG.secret);

    Ok(Json(serde_json::json!({"code": code})))
}

// edit

#[derive(Deserialize)]
pub struct EditQuery {
    page: Option<String>,
}

pub async fn edit_page(
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

    let page = PageContext::new()
        .insert("page_title", "Edit")
        .insert("slug", &slug)
        .insert("title", &title)
        .insert("body", &body)
        .insert("error", "");
    Ok(Html(page.render("edit.html")?))
}

#[derive(Deserialize)]
pub struct EditForm {
    slug: String,
    title: String,
    body: String,
}

pub async fn edit_post(
    UserToken(token): UserToken,
    State(state): State<Arc<AppState>>,
    Json(body): Json<EditForm>,
) -> Result<Json<serde_json::Value>, AppError> {
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

    let tx = state.db.begin_write()?;

    let mut pages_table = tx.open_table(PAGES)?;
    let existing = pages_table
        .get(body.slug.as_str())?
        .map(|g| g.value().clone());
    let meta = match existing {
        Some(existing_meta) => ResourceMeta {
            title: body.title.clone(),
            creator: existing_meta.creator.clone(),
            date: existing_meta.date,
            tags: existing_meta.tags.clone(),
        },
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

// helper
fn set_session_cookie(jar: CookieJar, username: impl AsRef<str>, secret: &str) -> CookieJar {
    let token = Signed::new(Session::new(username.as_ref())).generate(secret);
    let cookie = Cookie::build(("session", token))
        .path("/")
        .max_age(time::Duration::seconds(Session::EXPIRY_SECS))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build();
    jar.add(cookie)
}
