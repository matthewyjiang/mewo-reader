use mewo_server::build;
use mewo_server::cli::{self, Command};
use mewo_server::config::Config;
use std::io::{self, BufReader};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_env();
    match cli::parse(std::env::args().skip(1)) {
        Ok(Command::Serve) => serve(config).await,
        Ok(Command::Help) => print!("{}", cli::USAGE),
        Ok(Command::Admin(cmd)) => {
            let code = cli::run(
                cmd,
                &config,
                &mut BufReader::new(io::stdin()),
                &mut io::stdout(),
                &mut io::stderr(),
            )
            .await;
            std::process::exit(code);
        }
        Err(message) => {
            eprint!("{message}");
            std::process::exit(2);
        }
    }
}

async fn serve(config: Config) {
    let listen = config.listen.clone();
    tracing::info!(
        "auth: trusted_proxy_hops={}, password ops={}",
        config.auth.trusted_proxy_hops,
        config.auth.max_concurrent
    );
    tracing::info!(
        "limits: parses={}, min_free={}",
        config.limits.max_parses,
        config.limits.min_free_bytes
    );
    let app = build(config).await.expect("database");
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|err| panic!("bind {listen}: {err}"));
    tracing::info!("listening on {listen}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("serve");
}
