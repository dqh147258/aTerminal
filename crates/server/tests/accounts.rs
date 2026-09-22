use ai_terminal_security::{account::*, random_secret};
use reqwest::{Client, StatusCode};
struct Server {
    _dir: tempfile::TempDir,
    url: String,
    task: tokio::task::JoinHandle<()>,
    db: std::path::PathBuf,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort()
    }
}
async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("server.db");
    for user in ["alice", "bob"] {
        ai_terminal_server::account::manage_user(&db, user, "correct test password", false).unwrap()
    }
    let router = ai_terminal_server::router(&db, &random_secret().unwrap()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server {
        _dir: dir,
        url,
        task,
        db,
    }
}
async fn login(s: &Server, user: &str, platform: &str) -> Tokens {
    Client::new()
        .post(format!("{}/v2/auth/login", s.url))
        .json(&LoginRequest {
            username: user.into(),
            password: "correct test password".into(),
            device_name: platform.into(),
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
async fn devices(s: &Server, t: &Tokens) -> reqwest::Response {
    Client::new()
        .get(format!("{}/v2/devices", s.url))
        .bearer_auth(&t.access_token)
        .send()
        .await
        .unwrap()
}
#[tokio::test]
async fn accounts_are_isolated_and_grants_cannot_cross_accounts() {
    let s = server().await;
    let d = login(&s, "alice", "desktop").await;
    let a = login(&s, "alice", "ios").await;
    let b = login(&s, "bob", "android").await;
    let http = Client::new();
    http.post(format!("{}/v2/devices/heartbeat", s.url))
        .bearer_auth(&d.access_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(
        devices(&s, &a)
            .await
            .json::<Vec<Device>>()
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        devices(&s, &b)
            .await
            .json::<Vec<Device>>()
            .await
            .unwrap()
            .len(),
        1
    );
    let request = ConnectRequest {
        desktop_id: d.device_id.clone(),
    };
    assert_eq!(
        http.post(format!("{}/v2/connections", s.url))
            .bearer_auth(&b.access_token)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let grant = http
        .post(format!("{}/v2/connections", s.url))
        .bearer_auth(&a.access_token)
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<ConnectionGrant>()
        .await
        .unwrap();
    assert_eq!(grant.mobile_id, a.device_id);
    assert_eq!(
        http.delete(format!("{}/v2/devices/{}", s.url, d.device_id))
            .bearer_auth(&b.access_token)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    http.delete(format!("{}/v2/devices/{}", s.url, d.device_id))
        .bearer_auth(&a.access_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(devices(&s, &d).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        http.post(format!("{}/v2/auth/refresh", s.url))
            .json(&RefreshRequest {
                refresh_token: d.refresh_token
            })
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn refresh_is_atomic_and_logout_revokes_session() {
    let s = server().await;
    let a = login(&s, "alice", "ios").await;
    let http = Client::new();
    let url = format!("{}/v2/auth/refresh", s.url);
    let first = http
        .post(&url)
        .json(&RefreshRequest {
            refresh_token: a.refresh_token.clone(),
        })
        .send();
    let second = http
        .post(&url)
        .json(&RefreshRequest {
            refresh_token: a.refresh_token.clone(),
        })
        .send();
    let (first, second) = tokio::join!(first, second);
    let (first, second) = (first.unwrap(), second.unwrap());
    let (valid, invalid) = if first.status().is_success() {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    let next: Tokens = valid.json().await.unwrap();
    assert_eq!(devices(&s, &a).await.status(), StatusCode::UNAUTHORIZED);
    assert!(devices(&s, &next).await.status().is_success());
    http.post(format!("{}/v2/auth/logout", s.url))
        .bearer_auth(&next.access_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(devices(&s, &next).await.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn password_change_and_admin_reset_revoke_all_devices() {
    let s = server().await;
    let a = login(&s, "alice", "ios").await;
    let d = login(&s, "alice", "desktop").await;
    let http = Client::new();
    let url = format!("{}/v2/auth/password", s.url);
    assert_eq!(
        http.post(&url)
            .bearer_auth(&a.access_token)
            .json(&PasswordRequest {
                current_password: "incorrect".into(),
                new_password: "another long password".into()
            })
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    http.post(&url)
        .bearer_auth(&a.access_token)
        .json(&PasswordRequest {
            current_password: "correct test password".into(),
            new_password: "another long password".into(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(devices(&s, &a).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(devices(&s, &d).await.status(), StatusCode::UNAUTHORIZED);
    let b = login(&s, "bob", "android").await;
    ai_terminal_server::account::manage_user(&s.db, "bob", "new administrator password", true)
        .unwrap();
    assert_eq!(devices(&s, &b).await.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn unknown_user_login_is_limited() {
    let s = server().await;
    let req = LoginRequest {
        username: "unknown".into(),
        password: "wrong password".into(),
        device_name: "test".into(),
        platform: "ios".into(),
        public_key: DeviceIdentity::generate().unwrap().public,
    };
    let http = Client::new();
    for _ in 0..5 {
        assert_eq!(
            http.post(format!("{}/v2/auth/login", s.url))
                .json(&req)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        )
    }
    assert_eq!(
        http.post(format!("{}/v2/auth/login", s.url))
            .json(&req)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn connection_grants_expire_and_cannot_be_used_by_another_device() {
    let s = server().await;
    let desktop = login(&s, "alice", "desktop").await;
    let mobile = login(&s, "alice", "ios").await;
    let other = login(&s, "bob", "android").await;
    let http = Client::new();
    http.post(format!("{}/v2/devices/heartbeat", s.url))
        .bearer_auth(&desktop.access_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let grant: ConnectionGrant = http
        .post(format!("{}/v2/connections", s.url))
        .bearer_auth(&mobile.access_token)
        .json(&ConnectRequest {
            desktop_id: desktop.device_id,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let upgrade = |token: String| {
        http.get(format!("{}/v2/relay/{}/mobile", s.url, grant.room))
            .bearer_auth(token)
            .header("Connection", "upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
    };
    assert_eq!(
        upgrade(other.access_token).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let db = rusqlite::Connection::open(&s.db).unwrap();
    db.execute(
        "UPDATE connections SET expires_at=0 WHERE room=?1",
        [grant.room.clone()],
    )
    .unwrap();
    assert_eq!(
        upgrade(mobile.access_token).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
