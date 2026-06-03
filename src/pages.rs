use axum::{extract::State, http::StatusCode, response::Html};
use book::model::AppState;
use book::model::res::{FILES, PAGES};
use redb::{ReadableDatabase, ReadableTable};
use std::sync::Arc;
use tera::Context;

fn home_page(state: &AppState) -> Result<Html<String>, Box<dyn std::error::Error>> {
    let tx = state.db.begin_read()?;

    // 列出所有页面，按时间倒序
    let pages_table = tx.open_table(PAGES)?;
    let mut pages: Vec<(String, String, i64)> = Vec::new();
    for result in pages_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        pages.push((key.value().to_string(), r.title.clone(), r.date));
    }
    pages.sort_by(|a, b| b.2.cmp(&a.2));
    let pages: Vec<_> = pages.into_iter().map(|(n, t, _)| (n, t)).collect();

    // 列出所有文件，按时间倒序
    let files_table = tx.open_table(FILES)?;
    let mut files: Vec<(String, String, i64)> = Vec::new();
    for result in files_table.iter()? {
        let (key, value) = result?;
        let r = value.value();
        files.push((key.value().to_string(), r.title.clone(), r.date));
    }
    files.sort_by(|a, b| b.2.cmp(&a.2));
    let files: Vec<_> = files.into_iter().map(|(n, t, _)| (n, t)).collect();

    let mut ctx = Context::new();
    ctx.insert("site_title", &state.config.site_title);
    ctx.insert("base_url", &state.config.base_url);
    ctx.insert("page_title", "Home");
    ctx.insert("pages", &pages);
    ctx.insert("files", &files);

    let html = state.templates.render("home.html", &ctx)?;
    Ok(Html(html))
}

pub async fn home(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    home_page(&state).map_err(|e| {
        tracing::error!("{e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
