use axum::{Router, extract::DefaultBodyLimit, routing::get, routing::post};
use book::{CONFIG, model::AppState, model::init_tables};
use redb::Database;
use std::{path::Path, sync::Arc};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;

mod routes;

/// entry point
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().without_time().init();

    let db = init_db("data.redb").expect("failed to initialize database");
    let listener = TcpListener::bind(&CONFIG.server_addr).await.unwrap();

    let app = Router::new()
        // home / category
        .route("/", get(routes::home_page))
        .route("/category", get(routes::category_page))
        // account
        .route("/auth", get(routes::auth_page))
        .route("/auth/sign-in", post(routes::sign_in_post))
        .route("/auth/sign-up", post(routes::sign_up_post))
        .route("/auth/sign-out", get(routes::sign_out))
        .route("/profile", get(routes::profile_page))
        // edit / upload
        .route("/edit", get(routes::edit_page))
        .route("/edit", post(routes::edit_post))
        .route("/upload", get(routes::file_upload_page))
        .route("/upload", post(routes::file_upload_post))
        // view / download / delete
        .route("/{entry_id}/README.md", get(routes::entry_page))
        .route("/{entry_id}/delete", post(routes::entry_delete))
        .route("/{entry_id}/file/{file}", get(routes::file_download))
        .route("/{entry_id}/info/{file}", get(routes::file_info_page))
        .route("/{entry_id}/delete/{file}", post(routes::file_delete))
        // site export
        .route("/export", get(routes::export_zip))
        // everything else is served from embedded static assets
        .fallback(assets::static_handler);

    let app = app
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(CompressionLayer::new().zstd(true).gzip(true).deflate(true))
        .with_state(Arc::new(AppState { db }));

    let base_url = CONFIG.base_url.as_str();
    tracing::info!("🚀 Server started at: {base_url}");
    axum::serve(listener, app).await.unwrap();
}

/// open the database, creating and initializing tables on first run
fn init_db<P: AsRef<Path>>(path: P) -> Option<Database> {
    match path.as_ref().exists() {
        true => Database::open(path).ok(),
        false => init_tables(path).ok(),
    }
}

// static assets
//
// ++++++++++++============++++++++++++============++++++++++++============

/// static assets, embedded at compile time
mod assets {
    use axum::http::{StatusCode, Uri};
    use axum::response::{IntoResponse, Response};

    const FAVICON_SVG: &[u8] = include_bytes!("../assets/favicon.svg");
    const MODERN_NORMALIZE_CSS: &[u8] = include_bytes!("../assets/modern-normalize.css");
    const STYLE_CSS: &[u8] = include_bytes!("../assets/style-0019.css");
    const ALPINE_JS: &[u8] = include_bytes!("../assets/alpine.min.js");
    const ROBOTS_TXT: &[u8] = include_bytes!("../assets/robots.txt");

    /// serve embedded assets by path, 404 for anything else
    pub async fn static_handler(uri: Uri) -> Response {
        let (body, mime): (&'static [u8], &'static str) = match uri.path() {
            "/img/favicon.svg" => (FAVICON_SVG, "image/svg+xml"),
            "/css/modern-normalize.css" => (MODERN_NORMALIZE_CSS, "text/css"),
            "/css/style-0019.css" => (STYLE_CSS, "text/css"),
            "/js/alpine.min.js" => (ALPINE_JS, "text/javascript"),
            "/robots.txt" => (ROBOTS_TXT, "text/plain"),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        ([("content-type", mime)], body).into_response()
    }
}
