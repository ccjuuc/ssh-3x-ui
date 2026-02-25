mod handlers;
mod ssh_client;

use axum::Router;
use axum::routing::{get, post};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/", get(handlers::index))
        .route("/api/deploy", post(handlers::deploy))
        .route("/api/uninstall", post(handlers::uninstall))
        .route("/api/inbounds/add", post(handlers::add_inbound_handler))
        .route("/api/inbounds/list", post(handlers::list_inbounds_handler))
        .nest_service("/static", ServeDir::new("static"));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("服务启动于 http://{}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await.unwrap();
}
