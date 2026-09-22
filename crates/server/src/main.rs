use anyhow::{Context, Result};
fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "user") {
        anyhow::ensure!(
            args.len() == 4 && matches!(args[2].as_str(), "add" | "reset-password"),
            "usage: ai-terminal-server user add|reset-password USERNAME (AI_TERMINAL_DB selects database)"
        );
        let password = rpassword::prompt_password("Password (12+ bytes): ")?;
        anyhow::ensure!(
            password == rpassword::prompt_password("Confirm password: ")?,
            "passwords do not match"
        );
        let path = std::path::PathBuf::from(
            std::env::var("AI_TERMINAL_DB").unwrap_or_else(|_| "ai-terminal.sqlite3".into()),
        );
        ai_terminal_server::account::manage_user(
            &path,
            &args[3],
            &password,
            args[2] == "reset-password",
        )?;
        println!("User updated: {}", args[3]);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--healthcheck") {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:8787".parse()?,
            std::time::Duration::from_secs(1),
        )?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(1)))?;
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        anyhow::ensure!(response.starts_with("HTTP/1.1 200"), "unhealthy server");
        return Ok(());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(2)
        .build()?
        .block_on(run())
}
async fn run() -> Result<()> {
    let token = if let Ok(path) = std::env::var("AI_TERMINAL_ADMIN_TOKEN_FILE") {
        std::fs::read_to_string(path)?.trim().to_owned()
    } else {
        std::env::var("AI_TERMINAL_ADMIN_TOKEN")
            .context("set AI_TERMINAL_ADMIN_TOKEN_FILE or AI_TERMINAL_ADMIN_TOKEN")?
    };
    let path = std::path::PathBuf::from(
        std::env::var("AI_TERMINAL_DB").unwrap_or_else(|_| "ai-terminal.sqlite3".into()),
    );
    let router = ai_terminal_server::router(&path, &token)?;
    let bind = std::env::var("AI_TERMINAL_BIND").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!(
        "ai-terminal-server listening on {} (HTTP; use Compose TLS proxy for external access)",
        listener.local_addr()?
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
