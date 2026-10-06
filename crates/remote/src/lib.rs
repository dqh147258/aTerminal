//! Authenticated, encrypted transport. Public HTTP/WS requires explicit Debug app opt-in.
use ai_terminal_security::{HostIdentity, Invitation};
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{
        Message, client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig,
    },
};
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub mod account;
pub mod assistant;
mod channel;
mod tls;
pub use channel::{Channel, PathKind, StreamEvent};

#[derive(Clone, Serialize, Deserialize)]
pub struct HostPair {
    pub server: String,
    pub room: String,
    pub relay_token: String,
    pub identity: HostIdentity,
    pub read_only: bool,
    pub expires_at: u64,
    #[serde(default)]
    pub ice_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_ca_pem: Option<String>,
}
#[derive(Deserialize)]
struct PairResponse {
    room: String,
    desktop_token: String,
    mobile_token: String,
    expires_at: u64,
}
pub async fn create_pair(
    server: &str,
    admin: &str,
    read_only: bool,
) -> Result<(HostPair, Invitation)> {
    let base = validate_url(server)?;
    let server_ca_pem = std::env::var_os("AI_TERMINAL_CA_FILE")
        .map(std::fs::read_to_string)
        .transpose()
        .context("read configured relay CA")?;
    let pair: PairResponse = tls::http_client(server_ca_pem.as_deref())?
        .post(format!("{base}/v1/pairs"))
        .bearer_auth(admin.trim())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let identity = HostIdentity::generate()?;
    let ice_servers = std::env::var("AI_TERMINAL_ICE_SERVERS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_owned())
        .collect::<Vec<_>>();
    let invite = Invitation {
        version: 1,
        server: base.clone(),
        room: pair.room.clone(),
        relay_token: pair.mobile_token,
        desktop_public: identity.public.clone(),
        pairing_key: identity.pairing_key.clone(),
        ice_servers: ice_servers.clone(),
        server_ca_pem: server_ca_pem.clone(),
    };
    Ok((
        HostPair {
            server: base,
            room: pair.room,
            relay_token: pair.desktop_token,
            identity,
            read_only,
            expires_at: pair.expires_at,
            ice_servers,
            server_ca_pem,
        },
        invite,
    ))
}
static DEBUG_HTTP_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Android opts in only when its application build is debuggable.
/// Native optimization levels do not select the application's network policy.
/// Builds without the explicit debug-http feature always reject this opt-in.
pub fn set_debug_http_enabled(enabled: bool) {
    DEBUG_HTTP_ENABLED.store(
        enabled && cfg!(feature = "debug-http"),
        std::sync::atomic::Ordering::Relaxed,
    );
}

pub fn validate_url(server: &str) -> Result<String> {
    let url = reqwest::Url::parse(server)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("server URL cannot contain credentials, query, or fragment")
    }
    if url.scheme() != "https"
        && !(url.scheme() == "http"
            && (DEBUG_HTTP_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
                || matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
    {
        bail!("remote relay requires HTTPS; HTTP is allowed only on loopback")
    }
    Ok(server.trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod url_tests {
    use super::validate_url;

    #[test]
    fn relay_url_https_and_loopback_http_remain_supported() {
        for url in [
            "https://relay.example.com/",
            "http://localhost:8787",
            "http://127.0.0.1:8787",
            "http://[::1]:8787",
        ] {
            assert_eq!(validate_url(url).unwrap(), url.trim_end_matches('/'));
        }
    }

    #[test]
    fn relay_url_rejects_unsupported_schemes_and_embedded_credentials() {
        for url in [
            "ftp://relay.example.com",
            "ws://relay.example.com",
            "http://user:password@relay.example.com",
            "https://relay.example.com?token=value",
            "http://relay.example.com#fragment",
        ] {
            assert!(validate_url(url).is_err(), "accepted {url}");
        }
    }

    #[cfg(feature = "debug-http")]
    #[test]
    fn relay_url_http_requires_debug_application_opt_in() {
        assert!(validate_url("http://relay.example.com:8787").is_err());
        super::set_debug_http_enabled(true);
        for url in [
            "http://relay.example.com:8787",
            "http://198.51.100.12:8787",
            "http://192.168.0.36:7201",
            "http://[2001:db8::1]:8787",
        ] {
            assert_eq!(validate_url(url).unwrap(), url);
        }
        super::set_debug_http_enabled(false);
        assert!(validate_url("http://relay.example.com:8787").is_err());
    }

    #[cfg(not(feature = "debug-http"))]
    #[test]
    fn relay_url_builds_without_debug_http_reject_non_loopback_http() {
        super::set_debug_http_enabled(true);
        for url in [
            "http://relay.example.com:8787",
            "http://198.51.100.12:8787",
            "http://192.168.0.36:7201",
            "http://[2001:db8::1]:8787",
        ] {
            assert!(validate_url(url).is_err(), "accepted {url}");
        }
    }
}

async fn socket(
    server: &str,
    room: &str,
    role: &str,
    token: &str,
    ca: Option<&str>,
) -> Result<Socket> {
    socket_version(server, room, role, token, ca, 1).await
}
async fn socket_version(
    server: &str,
    room: &str,
    role: &str,
    token: &str,
    ca: Option<&str>,
    version: u32,
) -> Result<Socket> {
    let base = validate_url(server)?;
    anyhow::ensure!(
        room.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "invalid pair ID"
    );
    let url = format!(
        "{}/v{version}/relay/{room}/{role}",
        base.replacen("https:", "wss:", 1)
            .replacen("http:", "ws:", 1)
    );
    let mut request = url.into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    let config = WebSocketConfig::default()
        .max_message_size(Some(32768))
        .max_frame_size(Some(32768));
    let (socket, _) = tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect_async_tls_with_config(request, Some(config), true, tls::connector(ca)?),
    )
    .await
    .context("WebSocket connection timed out after 30 seconds")??;
    Ok(socket)
}
async fn wait_for_peer(socket: &mut Socket) -> Result<()> {
    tokio::time::timeout(CONNECT_TIMEOUT, ready(socket))
        .await
        .context("waiting for the other device timed out after 30 seconds")?
}

#[cfg(test)]
mod peer_join_tests {
    use super::{Socket, wait_for_peer};
    use futures_util::SinkExt;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream,
        tungstenite::{Message, protocol::Role},
    };

    async fn sockets() -> (Socket, WebSocketStream<TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        (
            WebSocketStream::from_raw_socket(MaybeTlsStream::Plain(client), Role::Client, None)
                .await,
            WebSocketStream::from_raw_socket(server, Role::Server, None).await,
        )
    }

    #[tokio::test]
    async fn peer_can_join_after_the_previous_fifteen_second_limit() {
        let (mut client, mut server) = sockets().await;
        tokio::time::pause();
        let waiting = tokio::spawn(async move { wait_for_peer(&mut client).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(16)).await;
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "a healthy slow peer was rejected");
        server.send(Message::Text("ready".into())).await.unwrap();
        waiting.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn missing_peer_times_out_with_peer_join_context() {
        let (mut client, _server) = sockets().await;
        tokio::time::pause();
        let waiting = tokio::spawn(async move { wait_for_peer(&mut client).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(29)).await;
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        tokio::time::advance(Duration::from_secs(2)).await;
        let error = waiting.await.unwrap().unwrap_err();
        assert!(error.is::<tokio::time::error::Elapsed>());
        assert!(error.to_string().contains("other device"));
    }
}

async fn ready(socket: &mut Socket) -> Result<()> {
    loop {
        match socket.next().await.context("relay disconnected")?? {
            Message::Text(text) if text == "ready" => return Ok(()),
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await?,
            Message::Pong(_) => {}
            _ => bail!("unexpected relay handshake"),
        }
    }
}
async fn binary(socket: &mut Socket) -> Result<Vec<u8>> {
    loop {
        match socket.next().await.context("relay disconnected")?? {
            Message::Binary(bytes) => return Ok(bytes.to_vec()),
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await?,
            Message::Pong(_) => {}
            _ => bail!("peer disconnected or restarted"),
        }
    }
}
