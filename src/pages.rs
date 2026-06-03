use axum::{extract::State, http::StatusCode, response::Html};
use book::model::{AppState, res::PAGES};
use redb::ReadableDatabase;
use std::sync::Arc;
use tera::Context;

fn home_page(state: &AppState) -> Result<Html<String>, Box<dyn std::error::Error>> {
    let tx = state.db.begin_read()?;
    let table = tx.open_table(PAGES)?;

    let (body, page_title) = match table.get("home")? {
        Some(resource) => {
            let r = resource.value();
            (r.data.render(), r.title.clone())
        }
        None => (
            "<h1>Welcome</h1><p>No content yet.</p>".to_string(),
            "Home".to_string(),
        ),
    };

    let mut ctx = Context::new();
    ctx.insert("site_title", &state.config.site_title);
    ctx.insert("page_title", &page_title);
    ctx.insert("body", &body);

    let html = state.templates.render("home.html", &ctx)?;
    Ok(Html(html))
}

pub async fn home(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    home_page(&state).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
