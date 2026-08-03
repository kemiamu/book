use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use book::CONFIG;
use book::model::{AppState, init_tables};
use redb::Database;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

mod routes;

/// entry point
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().without_time().init();

    let db = init_db("data.redb").expect("failed to initialize database");
    let listener = TcpListener::bind(&CONFIG.server_addr).await.unwrap();

    let app = Router::new()
        // home
        .route("/", get(routes::home_page))
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
        // view / download
        .route("/{entry}/README.md", get(routes::entry_page))
        .route("/tags/{tag}/README.md", get(routes::tags_page))
        .route("/{entry}/{file}", get(routes::file_download))
        .route("/{entry}/delete", post(routes::entry_delete))
        // static files
        .nest_service("/img", ServeDir::new("public/img"))
        .nest_service("/css", ServeDir::new("public/css"))
        .nest_service("/js", ServeDir::new("public/js"))
        .fallback_service(ServeDir::new(&CONFIG.site_root));

    let app = app
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(CompressionLayer::new().zstd(true).gzip(true).deflate(true))
        .with_state(Arc::new(AppState { db }));

    tracing::info!("🚀 Server started at: {}", &CONFIG.base_url);
    axum::serve(listener, app).await.unwrap();
}

/// open the database, creating and initializing tables on first run
fn init_db<P: AsRef<Path>>(path: P) -> Option<Database> {
    match path.as_ref().exists() {
        true => Database::open(path).ok(),
        false => init_tables(path).ok(),
    }
}
