use std::sync::Arc;
use std::time::Duration;

use toro_mcp::App;
use toro_mcp::config::Config;

#[tokio::main]
async fn main() {
    // scratch イメージには curl が無いので、HEALTHCHECK は自分自身で行う
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(if healthcheck().await { 0 } else { 1 });
    }

    let cfg = match Config::from_env() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(2);
        }
    };
    let port = cfg.port;
    let app = Arc::new(App::new(cfg).expect("HTTP client"));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind");
    eprintln!(
        "toro-mcp {} listening on :{port}",
        env!("CARGO_PKG_VERSION")
    );

    let server = axum::serve(listener, toro_mcp::routes::router(app))
        .with_graceful_shutdown(shutdown_signal());
    // SSE は客が切るまで終わらないので、待つのは 5 秒まで
    tokio::select! {
        result = server => result.expect("server"),
        _ = async {
            shutdown_signal().await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        } => {}
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

async fn healthcheck() -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let check = async {
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await?;
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut head = [0u8; 15];
        stream.read_exact(&mut head).await?;
        Ok::<bool, std::io::Error>(head.starts_with(b"HTTP/1.1 200"))
    };
    matches!(
        tokio::time::timeout(Duration::from_secs(3), check).await,
        Ok(Ok(true))
    )
}
