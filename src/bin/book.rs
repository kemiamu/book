use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use book::CONFIG;
use book::model::AppState;
mod routes;
use redb::Database;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, services::ServeDir};

/// entry point
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().without_time().init();

    let db = Database::open("data.redb").expect("failed to open database");
    let listener = TcpListener::bind(&CONFIG.server_addr).await.unwrap();

    let app = Router::new()
        .route("/", get(routes::home_page))
        .route("/page/{page}", get(routes::view_page))
        .route("/sign-in", get(routes::sign_in_page))
        .route("/sign-in", post(routes::sign_in_post))
        .route("/sign-up", get(routes::sign_up_page))
        .route("/sign-up", post(routes::sign_up_post))
        .route("/sign-out", get(routes::sign_out))
        .route("/profile", get(routes::profile_page))
        .route("/edit", get(routes::edit_page))
        .route("/edit", post(routes::edit_post))
        .route("/upload", get(routes::file_upload_page))
        .route("/upload", post(routes::file_upload_post))
        .route("/file/{slug}", get(routes::file_download))
        .fallback_service(ServeDir::new(&CONFIG.site_root));

    let app = app
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(CompressionLayer::new().zstd(true).gzip(true).deflate(true))
        .with_state(Arc::new(AppState { db }));

    tracing::info!("🚀 Server started at: {}", &CONFIG.base_url);
    axum::serve(listener, app).await.unwrap();
}
