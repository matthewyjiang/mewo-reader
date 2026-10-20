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
    let app = build(config).await.expect("database");
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|err| panic!("bind {listen}: {err}"));
    tracing::info!("listening on {listen}");
    axum::serve(listener, app).await.expect("serve");
}
