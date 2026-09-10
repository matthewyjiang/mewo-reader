use mewo_server::config::Config;
use mewo_server::build;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    let listen = config.listen.clone();
    let app = build(config).await.expect("database");
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|err| panic!("bind {listen}: {err}"));
    tracing::info!("listening on {listen}");
    axum::serve(listener, app).await.expect("serve");
}
