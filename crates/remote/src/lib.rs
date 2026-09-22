//! Authenticated, encrypted remote session transport. Local development alone permits ws://.
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
pub mod account;
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
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        bail!("remote relay requires HTTPS; HTTP is allowed only on loopback")
    }
    Ok(server.trim_end_matches('/').to_owned())
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
        Duration::from_secs(10),
        connect_async_tls_with_config(request, Some(config), true, tls::connector(ca)?),
    )
    .await??;
    Ok(socket)
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
