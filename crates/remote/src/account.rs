//! Serialized account state belongs in the platform credential vault, never in logs.
use ai_terminal_security::account::*;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct AccountSession {
    pub server: String,
    pub ca: Option<String>,
    pub identity: DeviceIdentity,
    pub tokens: Tokens,
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}
impl AccountSession {
    pub async fn login(
        server: &str,
        ca: Option<String>,
        username: String,
        password: String,
        device_name: String,
        platform: String,
    ) -> Result<Self> {
        let server = super::validate_url(server)?;
        let identity = DeviceIdentity::generate()?;
        let req = LoginRequest {
            username,
            password,
            device_name,
            platform,
            public_key: identity.public.clone(),
        };
        let response = super::tls::http_client(ca.as_deref())?
            .post(format!("{server}/v2/auth/login"))
            .json(&req)
            .send()
            .await?
            .error_for_status()?;
        ensure!(
            !response.status().is_redirection(),
            "login server returned an HTTP redirect ({}); check the server address and port",
            response.status()
        );
        let tokens = response
            .json()
            .await
            .context("invalid login response; check the server address and port")?;
        Ok(Self {
            server,
            ca,
            identity,
            tokens,
            pins: BTreeMap::new(),
        })
    }
    pub async fn renew(&mut self) -> Result<bool> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
        if self.tokens.expires_at > now + 60 {
            return Ok(false);
        }
        let tokens: Tokens = super::tls::http_client(self.ca.as_deref())?
            .post(format!("{}/v2/auth/refresh", self.server))
            .json(&RefreshRequest {
                refresh_token: self.tokens.refresh_token.clone(),
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            tokens.device_id == self.tokens.device_id && tokens.username == self.tokens.username,
            "refresh changed account identity"
        );
        self.tokens = tokens;
        Ok(true)
    }
    fn client(&self) -> Result<reqwest::Client> {
        super::tls::http_client(self.ca.as_deref())
    }
    pub async fn devices(&mut self) -> Result<Vec<Device>> {
        self.renew().await?;
        self.client()?
            .get(format!("{}/v2/devices", self.server))
            .bearer_auth(&self.tokens.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }
    pub async fn heartbeat(&mut self) -> Result<Vec<ConnectionGrant>> {
        self.renew().await?;
        self.post("devices/heartbeat", &()).await
    }
    pub async fn connect(&mut self, desktop_id: String) -> Result<ConnectionGrant> {
        self.renew().await?;
        let grant: ConnectionGrant = self
            .post(
                "connections",
                &ConnectRequest {
                    desktop_id: desktop_id.clone(),
                },
            )
            .await?;
        ensure!(
            grant.desktop_id == desktop_id
                && grant.mobile_id == self.tokens.device_id
                && grant.mobile_public == self.identity.public
                && grant.version == 2,
            "invalid connection grant"
        );
        self.pin(&grant.desktop_id, &grant.desktop_public)?;
        Ok(grant)
    }
    pub fn pin(&mut self, id: &str, public: &str) -> Result<()> {
        validate_public(public)?;
        if let Some(known) = self.pins.get(id) {
            ensure!(
                known == public,
                "device public key changed; revoke and enroll the device again"
            )
        }
        self.pins.insert(id.into(), public.into());
        Ok(())
    }
    pub async fn revoke(&mut self, id: &str) -> Result<()> {
        ensure!(
            id.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
            "invalid device id"
        );
        self.renew().await?;
        self.client()?
            .delete(format!("{}/v2/devices/{id}", self.server))
            .bearer_auth(&self.tokens.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
    pub async fn logout(&mut self) -> Result<()> {
        self.renew().await?;
        self.empty_post("auth/logout", &()).await
    }
    pub async fn change_password(
        &mut self,
        current_password: String,
        new_password: String,
    ) -> Result<()> {
        self.renew().await?;
        self.empty_post(
            "auth/password",
            &PasswordRequest {
                current_password,
                new_password,
            },
        )
        .await
    }
    async fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T> {
        self.client()?
            .post(format!("{}/v2/{path}", self.server))
            .bearer_auth(&self.tokens.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .map_err(Into::into)
    }
    async fn empty_post(&self, path: &str, body: &impl Serialize) -> Result<()> {
        self.client()?
            .post(format!("{}/v2/{path}", self.server))
            .bearer_auth(&self.tokens.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AccountSession;
    use axum::{Router, http::StatusCode, routing::post};

    #[tokio::test]
    async fn login_reports_redirects_and_html_from_the_wrong_server_route() {
        for (status, expected) in [
            (StatusCode::MOVED_PERMANENTLY, "HTTP redirect"),
            (StatusCode::OK, "invalid login response"),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().route(
                "/v2/auth/login",
                post(move || async move { (status, "<html>Not the login API</html>") }),
            );
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let result = AccountSession::login(
                &url,
                None,
                "test-user".into(),
                "test-password".into(),
                "test-device".into(),
                "android".into(),
            )
            .await;
            server.abort();
            let error = result.err().expect("wrong route must not authenticate");
            assert!(error.to_string().contains(expected), "{error:#}");
            assert!(error.to_string().contains("server address and port"));
        }
    }
}
