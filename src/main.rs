use axum::{Router, extract::DefaultBodyLimit, routing::get, routing::post};
use book::{CONFIG, model::AppState, model::init_tables};
use redb::Database;
use std::{path::Path, sync::Arc};
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, services::ServeDir};

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
        // everything else falls through to static files
        .fallback_service(ServeDir::new(&CONFIG.site_root));

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
