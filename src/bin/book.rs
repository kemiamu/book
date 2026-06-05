use axum::Router;
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
    tracing_subscriber::fmt().init();

    let db = Database::open("data.redb").expect("failed to open database");
    let listener = TcpListener::bind(&CONFIG.server_addr).await.unwrap();

    let app = Router::new()
        .route("/", get(routes::home_page))
        .route("/view/{page}", get(routes::view_page))
        .route("/sign-in", get(routes::sign_in_page))
        .route("/sign-in", post(routes::sign_in_post))
        .route("/sign-up", get(routes::sign_up_page))
        .route("/sign-up", post(routes::sign_up_post))
        .route("/sign-out", get(routes::sign_out))
        .route("/profile", get(routes::profile_page))
        .route("/profile/invite", get(routes::generate_invite))
        .route("/edit", get(routes::edit_page))
        .route("/edit", post(routes::edit_post))
        .route("/upload", get(routes::file_upload_page))
        .route("/upload", post(routes::file_upload_post))
        .route("/file/{slug}", get(routes::file_download));

    let app = app
        .fallback_service(ServeDir::new(&CONFIG.site_root))
        .layer(CompressionLayer::new().zstd(true).gzip(true).deflate(true));

    tracing::info!("🚀 Server started at: {}", &CONFIG.base_url);
    let app = app.with_state(Arc::new(AppState { db }));
    axum::serve(listener, app).await.unwrap();
}
