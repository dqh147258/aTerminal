use ai_terminal_security::account::{
    ConnectRequest, ConnectionGrant, DeviceIdentity, LoginRequest, RefreshRequest, Tokens,
};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

struct Server {
    _dir: tempfile::TempDir,
    db: std::path::PathBuf,
    url: String,
    token: String,
    http: Client,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("admin.sqlite3");
        let token = ai_terminal_security::random_secret().unwrap();
        let router = ai_terminal_server::router(&db, &token).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            _dir: dir,
            db,
            url,
            token,
            http: Client::new(),
            task,
        }
    }
    fn admin(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/v2/admin/{path}", self.url))
            .bearer_auth(&self.token)
            .header("X-Admin-Request", "1")
    }
    async fn get(&self, path: &str) -> Value {
        self.admin(Method::GET, path)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn create(&self, name: &str) -> Value {
        let response = self
            .admin(Method::POST, "users")
            .json(&json!({"username":name,"password":"correct test password"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        response.json().await.unwrap()
    }
    async fn login(&self, user: &str, platform: &str) -> Tokens {
        self.http
            .post(format!("{}/v2/auth/login", self.url))
            .json(&LoginRequest {
                username: user.into(),
                password: "correct test password".into(),
                device_name: format!("<img src=x onerror=alert(1)> {platform}"),
                platform: platform.into(),
                public_key: DeviceIdentity::generate().unwrap().public,
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn grant(&self, desktop: &Tokens, mobile: &Tokens) -> ConnectionGrant {
        self.http
            .post(format!("{}/v2/devices/heartbeat", self.url))
            .bearer_auth(&desktop.access_token)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        self.http
            .post(format!("{}/v2/connections", self.url))
            .bearer_auth(&mobile.access_token)
            .json(&ConnectRequest {
                desktop_id: desktop.device_id.clone(),
            })
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn socket(
        &self,
        grant: &ConnectionGrant,
        token: &Tokens,
        role: &str,
    ) -> reqwest::Upgraded {
        let response = self
            .http
            .get(format!("{}/v2/relay/{}/{role}", self.url, grant.room))
            .bearer_auth(&token.access_token)
            .header("Connection", "upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        response.upgrade().await.unwrap()
    }
    async fn denied(&self, tokens: &Tokens) {
        assert_eq!(
            self.http
                .get(format!("{}/v2/devices", self.url))
                .bearer_auth(&tokens.access_token)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            self.http
                .post(format!("{}/v2/auth/refresh", self.url))
                .json(&RefreshRequest {
                    refresh_token: tokens.refresh_token.clone()
                })
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

async fn closed(mut socket: reqwest::Upgraded) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        // These test sockets receive only the small unmasked ready/close frames.
        let mut bytes = Vec::new();
        loop {
            let mut buf = [0; 64];
            let n = socket.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.windows(2).any(|v| v == [0x88, 0]) {
                break;
            }
        }
    })
    .await
    .expect("revoked websocket remained open");
}

#[tokio::test]
async fn all_admin_routes_require_auth_and_reject_cross_site_requests() {
    let s = Server::start().await;
    for (method, path) in [
        (Method::GET, "overview"),
        (Method::GET, "users"),
        (Method::POST, "users"),
        (Method::POST, "users/no-user/password"),
        (Method::GET, "devices"),
        (Method::DELETE, "devices/no-device"),
        (Method::GET, "connections"),
        (Method::DELETE, "connections/no-room"),
    ] {
        let response = s
            .http
            .request(method.clone(), format!("{}/v2/admin/{path}", s.url))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            s.admin(method, path)
                .header("Origin", "https://evil.invalid")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        s.http
            .get(format!("{}/v2/admin/users", s.url))
            .bearer_auth(&s.token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    for site in ["same-site", "cross-site"] {
        assert_eq!(
            s.admin(Method::GET, "overview")
                .header("Sec-Fetch-Site", site)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        s.admin(Method::GET, "overview")
            .header("Origin", &s.url)
            .header("Sec-Fetch-Site", "same-origin")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        s.admin(Method::GET, "overview")
            .header("Origin", "null")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let preflight = s
        .http
        .request(Method::OPTIONS, format!("{}/v2/admin/users", s.url))
        .header("Origin", "https://evil.invalid")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert!(
        !preflight
            .headers()
            .contains_key("access-control-allow-origin")
    );
    let page = s
        .http
        .get(format!("{}/admin/", s.url))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
    assert!(page.text().await.unwrap().contains("管理控制台"));
}

#[tokio::test]
async fn validation_paging_filters_and_metadata_do_not_leak_secrets() {
    let s = Server::start().await;
    for body in [
        json!({"username":"bad name","password":"correct test password"}),
        json!({"username":"alice","password":"short"}),
        json!({"username":"alice","password":"correct test password","admin":true}),
    ] {
        assert!(matches!(
            s.admin(Method::POST, "users")
                .json(&body)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
        ));
    }
    let alice = s.create("alice").await;
    s.create("bob").await;
    assert_eq!(
        s.admin(Method::POST, "users")
            .json(&json!({"username":"alice","password":"correct test password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        s.get("users?limit=1&offset=1").await["items"][0]["username"],
        "bob"
    );
    assert_eq!(s.get("users?q=ALIce").await["total"], 1);
    for query in [
        "limit=0",
        "limit=101",
        "offset=1000001",
        "offset=-1",
        "unexpected=1",
    ] {
        assert_eq!(
            s.admin(Method::GET, &format!("users?{query}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let desktop = s.login("alice", "desktop").await;
    let mobile = s.login("alice", "ios").await;
    s.login("bob", "android").await;
    let grant = s.grant(&desktop, &mobile).await;
    let user = alice["id"].as_str().unwrap();
    let devices = s.get(&format!("devices?user_id={user}")).await;
    assert_eq!(devices["total"], 2);
    assert_eq!(s.get("devices?q=%3Cimg").await["total"], 3);
    let connections = s.get(&format!("connections?user_id={user}")).await;
    assert_eq!(connections["total"], 1);
    assert_eq!(connections["items"][0]["id"], grant.room);
    let overview = s.get("overview").await;
    assert_eq!(overview["users"], 2);
    assert_eq!(overview["devices"], 3);
    assert_eq!(overview["active_connections"], 1);
    let output = json!([s.get("users").await, devices, connections, overview]).to_string();
    for forbidden in [
        "password",
        "public_key",
        "grant_json",
        "access_token",
        "refresh_token",
        "$argon2",
        &desktop.access_token,
        &mobile.refresh_token,
        &grant.desktop_public,
        &grant.mobile_public,
    ] {
        assert!(!output.contains(forbidden), "leaked {forbidden}");
    }
    let db = rusqlite::Connection::open(&s.db).unwrap();
    db.execute("UPDATE connections SET expires_at=0", [])
        .unwrap();
    assert_eq!(s.get("connections").await["items"][0]["status"], "expired");
    assert_eq!(s.get("overview").await["active_connections"], 0);
}

#[tokio::test]
async fn reset_password_closes_relays_and_invalidates_all_user_sessions() {
    let s = Server::start().await;
    let alice = s.create("alice").await;
    s.create("bob").await;
    let desktop = s.login("alice", "desktop").await;
    let mobile = s.login("alice", "ios").await;
    let bob = s.login("bob", "android").await;
    let grant = s.grant(&desktop, &mobile).await;
    let a = s.socket(&grant, &desktop, "desktop").await;
    let b = s.socket(&grant, &mobile, "mobile").await;
    let path = format!("users/{}/password", alice["id"].as_str().unwrap());
    assert_eq!(
        s.admin(Method::POST, &path)
            .json(&json!({"password":"new correct password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    closed(a).await;
    closed(b).await;
    s.denied(&desktop).await;
    s.denied(&mobile).await;
    assert_eq!(
        s.http
            .get(format!("{}/v2/devices", s.url))
            .bearer_auth(&bob.access_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(s.get("connections").await["items"][0]["status"], "revoked");
    assert_eq!(s.get("overview").await["online_devices"], 0);
    let login = |password: &str| {
        s.http
            .post(format!("{}/v2/auth/login", s.url))
            .json(&LoginRequest {
                username: "alice".into(),
                password: password.into(),
                device_name: "test".into(),
                platform: "ios".into(),
                public_key: DeviceIdentity::generate().unwrap().public,
            })
    };
    assert_eq!(
        login("correct test password")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login("new correct password").send().await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn device_and_connection_revocation_are_persistent_and_scoped() {
    let s = Server::start().await;
    s.create("alice").await;
    let desktop = s.login("alice", "desktop").await;
    let mobile = s.login("alice", "ios").await;
    let grant = s.grant(&desktop, &mobile).await;
    let a = s.socket(&grant, &desktop, "desktop").await;
    let b = s.socket(&grant, &mobile, "mobile").await;
    let untouched = s.grant(&desktop, &mobile).await;
    let other_desktop = s.socket(&untouched, &desktop, "desktop").await;
    let other_mobile = s.socket(&untouched, &mobile, "mobile").await;
    assert_eq!(
        s.admin(Method::DELETE, &format!("connections/{}", grant.room))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    closed(a).await;
    closed(b).await;
    let other = s.get(&format!("connections?q={}", untouched.room)).await;
    assert_eq!(other["items"][0]["status"], "connected");
    assert_eq!(other["items"][0]["revoked"], false);
    assert_eq!(
        s.http
            .get(format!("{}/v2/devices", s.url))
            .bearer_auth(&mobile.access_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let next = s.grant(&desktop, &mobile).await;
    let a = s.socket(&next, &desktop, "desktop").await;
    let b = s.socket(&next, &mobile, "mobile").await;
    assert_eq!(
        s.admin(Method::DELETE, &format!("devices/{}", desktop.device_id))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    closed(a).await;
    closed(b).await;
    closed(other_desktop).await;
    closed(other_mobile).await;
    s.denied(&desktop).await;
    assert_eq!(
        s.http
            .get(format!("{}/v2/devices", s.url))
            .bearer_auth(&mobile.access_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        s.get("devices?q=desktop").await["items"][0]["revoked"],
        true
    );
    let db = rusqlite::Connection::open(&s.db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM connections WHERE revoked=0",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    for path in ["devices/missing", "connections/missing"] {
        assert_eq!(
            s.admin(Method::DELETE, path).send().await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        s.admin(Method::POST, "users/missing/password")
            .json(&json!({"password":"new correct password"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
