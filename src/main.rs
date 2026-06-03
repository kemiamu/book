use axum::{Router, routing::get};
use book::config::Config;
use book::model::AppState;
use redb::Database;
use std::sync::Arc;
use tera::Tera;
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, services::ServeDir};
mod pages;

#[tokio::main]
async fn main() {
    let config = Config::init("server.toml").expect("failed to load config");
    let db = Database::open("data.redb").expect("failed to open database");
    let templates = Tera::new("templates/**/*").expect("failed to load templates");
    let listener = TcpListener::bind(&config.server_addr).await.unwrap();

    // home page
    let app = Router::new().route("/", get(pages::home));

    let app = app
        .fallback_service(ServeDir::new(&config.site_root))
        .layer(CompressionLayer::new().zstd(true).gzip(true).deflate(true));

    println!("🚀 Server started at: {}", &config.base_url);
    let app = app.with_state(Arc::new(AppState {
        config,
        db,
        templates,
    }));
    axum::serve(listener, app).await.unwrap();
}
