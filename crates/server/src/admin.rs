//! Same-origin administrator UI and deliberately restricted metadata APIs.
use super::*;
use axum::{
    extract::{Query, Request},
    middleware::{self, Next},
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;

type Api<T> = std::result::Result<T, StatusCode>;
fn internal(_: impl std::fmt::Display) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}

pub(super) fn routes(app: App) -> Router<App> {
    let api = Router::new()
        .route("/v2/admin/overview", get(overview))
        .route("/v2/admin/users", get(users).post(create_user))
        .route("/v2/admin/users/{id}/password", post(reset_password))
        .route("/v2/admin/devices", get(devices))
        .route("/v2/admin/devices/{id}", delete(revoke_device))
        .route("/v2/admin/connections", get(connections))
        .route("/v2/admin/connections/{id}", delete(revoke_connection))
        .route_layer(middleware::from_fn_with_state(app, authorize));
    Router::new()
        .merge(api)
        .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
        .route(
            "/admin/",
            get(|| async {
                asset(
                    "text/html; charset=utf-8",
                    include_str!("../admin/index.html"),
                )
            }),
        )
        .route(
            "/admin/app.css",
            get(|| async { asset("text/css; charset=utf-8", include_str!("../admin/app.css")) }),
        )
        .route(
            "/admin/app.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_str!("../admin/app.js"),
                )
            }),
        )
        .route(
            "/admin/lucide.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_str!("../admin/lucide.js"),
                )
            }),
        )
        .route(
            "/admin/LUCIDE-LICENSE",
            get(|| async {
                asset(
                    "text/plain; charset=utf-8",
                    include_str!("../admin/LUCIDE-LICENSE"),
                )
            }),
        )
        .layer(middleware::from_fn(security_headers))
}

fn asset(content_type: &'static str, body: &'static str) -> impl IntoResponse {
    ([("content-type", content_type)], body)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    ] {
        response.headers_mut().insert(key, value.parse().unwrap());
    }
    response
}

async fn authorize(State(app): State<App>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !admin(&app, headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // A non-simple header plus no CORS support prevents cross-site form requests.
    // Fetch Metadata also rejects cross-site requests even if a token was supplied.
    if headers.get("x-admin-request").and_then(|v| v.to_str().ok()) != Some("1")
        || headers
            .get("sec-fetch-site")
            .is_some_and(|v| v != "same-origin" && v != "none")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = headers.get("origin") {
        let valid = origin
            .to_str()
            .ok()
            .and_then(|v| v.parse::<axum::http::Uri>().ok())
            .is_some_and(|uri| {
                matches!(uri.scheme_str(), Some("http" | "https"))
                    && uri.authority().is_some_and(|a| {
                        headers.get("host").and_then(|h| h.to_str().ok()) == Some(a.as_str())
                    })
                    && uri.path() == "/"
                    && uri.query().is_none()
            });
        if !valid {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}

async fn blocking<T: Send + 'static>(
    app: App,
    f: impl FnOnce(&mut Connection) -> Api<T> + Send + 'static,
) -> Api<T> {
    tokio::task::spawn_blocking(move || f(&mut *app.db.lock().map_err(internal)?))
        .await
        .map_err(internal)?
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Filter {
    #[serde(default)]
    q: String,
    #[serde(default)]
    user_id: String,
    limit: Option<u32>,
    #[serde(default)]
    offset: u32,
}
impl Filter {
    fn validate(&self) -> Api<()> {
        if self.q.len() > 128
            || self.user_id.len() > 128
            || !(1..=100).contains(&self.limit.unwrap_or(25))
            || self.offset > 1_000_000
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(())
    }
    fn limit(&self) -> u32 {
        self.limit.unwrap_or(25)
    }
}
#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    total: i64,
    offset: u32,
    limit: u32,
}
#[derive(Serialize)]
struct User {
    id: String,
    username: String,
    device_count: i64,
}
#[derive(Serialize)]
struct Device {
    id: String,
    user_id: String,
    username: String,
    name: String,
    platform: String,
    last_seen: i64,
    revoked: bool,
    online: bool,
}
#[derive(Serialize)]
struct AdminConnection {
    id: String,
    user_id: String,
    username: String,
    desktop_name: String,
    mobile_name: String,
    expires_at: i64,
    lease_expiry: i64,
    revoked: bool,
    status: &'static str,
    desktop_connected: bool,
    mobile_connected: bool,
}
#[derive(Serialize)]
struct Overview {
    users: i64,
    devices: i64,
    online_devices: i64,
    active_connections: i64,
    server_time: i64,
}

async fn overview(State(app): State<App>) -> Api<Json<Overview>> {
    blocking(app, |db| {
        let current = now();
        let users = db.query_row("SELECT count(*) FROM users", [], |r| r.get(0)).map_err(internal)?;
        let devices = db.query_row("SELECT count(*) FROM devices WHERE revoked=0", [], |r| r.get(0)).map_err(internal)?;
        let online_devices = db.query_row("SELECT count(*) FROM devices d WHERE revoked=0 AND last_seen>?1 AND EXISTS(SELECT 1 FROM auth_sessions s WHERE s.device_id=d.id AND s.refresh_expiry>?2)", params![current-15,current], |r| r.get(0)).map_err(internal)?;
        let active_connections = db.query_row("SELECT count(*) FROM connections c WHERE revoked=0 AND lease_expiry>?1 AND (expires_at>?1 OR (desktop_used=1 AND mobile_used=1)) AND EXISTS(SELECT 1 FROM devices d JOIN auth_sessions s ON s.device_id=d.id WHERE d.id=c.desktop_id AND d.revoked=0 AND s.refresh_expiry>?1) AND EXISTS(SELECT 1 FROM devices d JOIN auth_sessions s ON s.device_id=d.id WHERE d.id=c.mobile_id AND d.revoked=0 AND s.refresh_expiry>?1)", [current], |r| r.get(0)).map_err(internal)?;
        Ok(Json(Overview { users, devices, online_devices, active_connections, server_time: current }))
    }).await
}

async fn users(State(app): State<App>, Query(f): Query<Filter>) -> Api<Json<Page<User>>> {
    f.validate()?;
    blocking(app, move |db| {
        let total = db.query_row("SELECT count(*) FROM users WHERE instr(lower(username),lower(?1))>0", [&f.q], |r| r.get(0)).map_err(internal)?;
        let mut stmt = db.prepare("SELECT u.id,u.username,(SELECT count(*) FROM devices d WHERE d.user_id=u.id AND d.revoked=0) FROM users u WHERE instr(lower(username),lower(?1))>0 ORDER BY u.username,u.id LIMIT ?2 OFFSET ?3").map_err(internal)?;
        let items = stmt.query_map(params![f.q,f.limit(),f.offset], |r| Ok(User { id:r.get(0)?, username:r.get(1)?, device_count:r.get(2)? })).map_err(internal)?.collect::<rusqlite::Result<_>>().map_err(internal)?;
        Ok(Json(Page {items,total,offset:f.offset,limit:f.limit()}))
    }).await
}

async fn devices(State(app): State<App>, Query(f): Query<Filter>) -> Api<Json<Page<Device>>> {
    f.validate()?;
    blocking(app, move |db| {
        let predicate = " FROM devices d JOIN users u ON u.id=d.user_id WHERE (?1='' OR d.user_id=?1) AND (instr(lower(d.name),lower(?2))>0 OR instr(lower(u.username),lower(?2))>0 OR instr(lower(d.id),lower(?2))>0)";
        let total = db.query_row(&format!("SELECT count(*){predicate}"), params![f.user_id,f.q], |r| r.get(0)).map_err(internal)?;
        let mut stmt = db.prepare(&format!("SELECT d.id,d.user_id,u.username,d.name,d.platform,d.last_seen,d.revoked,(d.revoked=0 AND d.last_seen>?3 AND EXISTS(SELECT 1 FROM auth_sessions s WHERE s.device_id=d.id AND s.refresh_expiry>?4)){predicate} ORDER BY d.revoked,d.last_seen DESC,d.id LIMIT ?5 OFFSET ?6")).map_err(internal)?;
        let items = stmt.query_map(params![f.user_id,f.q,now()-15,now(),f.limit(),f.offset], |r| Ok(Device {id:r.get(0)?,user_id:r.get(1)?,username:r.get(2)?,name:r.get(3)?,platform:r.get(4)?,last_seen:r.get(5)?,revoked:r.get(6)?,online:r.get(7)?})).map_err(internal)?.collect::<rusqlite::Result<_>>().map_err(internal)?;
        Ok(Json(Page {items,total,offset:f.offset,limit:f.limit()}))
    }).await
}

async fn connections(
    State(app): State<App>,
    Query(f): Query<Filter>,
) -> Api<Json<Page<AdminConnection>>> {
    f.validate()?;
    let rooms = app.rooms.clone();
    blocking(app, move |db| {
        let predicate = " FROM connections c JOIN devices d ON d.id=c.desktop_id JOIN devices m ON m.id=c.mobile_id JOIN users u ON u.id=d.user_id WHERE (?1='' OR d.user_id=?1) AND (instr(lower(u.username),lower(?2))>0 OR instr(lower(d.name),lower(?2))>0 OR instr(lower(m.name),lower(?2))>0 OR instr(lower(c.room),lower(?2))>0)";
        let total = db.query_row(&format!("SELECT count(*){predicate}"), params![f.user_id,f.q], |r| r.get(0)).map_err(internal)?;
        let mut stmt = db.prepare(&format!("SELECT c.room,d.user_id,u.username,d.name,m.name,c.expires_at,c.lease_expiry,c.revoked,c.desktop_used,c.mobile_used{predicate} ORDER BY c.revoked,c.lease_expiry DESC,c.room LIMIT ?3 OFFSET ?4")).map_err(internal)?;
        let rooms = rooms.lock().map_err(internal)?;
        let items = stmt.query_map(params![f.user_id,f.q,f.limit(),f.offset], |r| {
            let id: String = r.get(0)?;
            let expires_at: i64 = r.get(5)?;
            let lease_expiry: i64 = r.get(6)?;
            let revoked: bool = r.get(7)?;
            let used = r.get::<_,bool>(8)? && r.get::<_,bool>(9)?;
            let slots = rooms.get(&id);
            let desktop_connected = slots.is_some_and(|s| s.desktop.is_some());
            let mobile_connected = slots.is_some_and(|s| s.mobile.is_some());
            let status = if revoked { "revoked" } else if lease_expiry<=now() || (!used && expires_at<=now()) { "expired" } else if !account::allowed(db,&id) { "invalid" } else if desktop_connected && mobile_connected { "connected" } else { "waiting" };
            Ok(AdminConnection {id,user_id:r.get(1)?,username:r.get(2)?,desktop_name:r.get(3)?,mobile_name:r.get(4)?,expires_at,lease_expiry,revoked,status,desktop_connected,mobile_connected})
        }).map_err(internal)?.collect::<rusqlite::Result<_>>().map_err(internal)?;
        Ok(Json(Page {items,total,offset:f.offset,limit:f.limit()}))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUser {
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetPassword {
    password: String,
}

async fn hash(app: &App, password: String) -> Api<String> {
    if !(12..=1024).contains(&password.len()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        account::password_hash(&password).map_err(internal)
    })
    .await
    .map_err(internal)?
}

async fn create_user(
    State(app): State<App>,
    Json(req): Json<CreateUser>,
) -> Api<(StatusCode, Json<User>)> {
    account::username(&req.username)?;
    let password = hash(&app, req.password).await?;
    blocking(app, move |db| {
        let id = ai_terminal_security::random_secret().map_err(internal)?;
        let changed = db.execute("INSERT INTO users(id,username,password) VALUES (?1,?2,?3) ON CONFLICT(username) DO NOTHING",params![id,req.username,password]).map_err(internal)?;
        if changed==0 { return Err(StatusCode::CONFLICT); }
        Ok((StatusCode::CREATED,Json(User {id,username:req.username,device_count:0})))
    }).await
}

// Called after committing revocation. The relay's periodic DB check also covers
// saturated send queues and upgrades racing with the transaction.
fn disconnect(app: &App, rooms: Vec<String>) {
    let mut live = app.rooms.lock().unwrap();
    for room in rooms {
        if let Some(slots) = live.remove(&room) {
            for (_, sender) in slots.desktop.into_iter().chain(slots.mobile) {
                let _ = sender.try_send(Message::Close(None));
            }
        }
    }
}
fn user_rooms(db: &Connection, id: &str) -> Api<Vec<String>> {
    let mut stmt = db.prepare("SELECT room FROM connections WHERE desktop_id IN (SELECT id FROM devices WHERE user_id=?1) OR mobile_id IN (SELECT id FROM devices WHERE user_id=?1)").map_err(internal)?;
    stmt.query_map([id], |r| r.get(0))
        .map_err(internal)?
        .collect::<rusqlite::Result<_>>()
        .map_err(internal)
}

async fn reset_password(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(req): Json<ResetPassword>,
) -> Api<StatusCode> {
    let password = hash(&app, req.password).await?;
    let rooms = blocking(app.clone(), move |db| {
        let tx = db.transaction().map_err(internal)?;
        if tx.execute("UPDATE users SET password=?1 WHERE id=?2",params![password,id]).map_err(internal)?!=1 { return Err(StatusCode::NOT_FOUND); }
        let rooms = user_rooms(&tx,&id)?;
        tx.execute("DELETE FROM auth_sessions WHERE device_id IN (SELECT id FROM devices WHERE user_id=?1)",[&id]).map_err(internal)?;
        tx.execute("UPDATE connections SET revoked=1 WHERE desktop_id IN (SELECT id FROM devices WHERE user_id=?1) OR mobile_id IN (SELECT id FROM devices WHERE user_id=?1)",[&id]).map_err(internal)?;
        tx.execute("UPDATE devices SET last_seen=0 WHERE user_id=?1",[&id]).map_err(internal)?;
        tx.commit().map_err(internal)?;
        Ok(rooms)
    }).await?;
    disconnect(&app, rooms);
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_device(State(app): State<App>, Path(id): Path<String>) -> Api<StatusCode> {
    let rooms = blocking(app.clone(), move |db| {
        let tx = db.transaction().map_err(internal)?;
        if tx
            .execute("UPDATE devices SET revoked=1 WHERE id=?1", [&id])
            .map_err(internal)?
            != 1
        {
            return Err(StatusCode::NOT_FOUND);
        }
        let rooms = {
            let mut stmt = tx
                .prepare("SELECT room FROM connections WHERE desktop_id=?1 OR mobile_id=?1")
                .map_err(internal)?;
            stmt.query_map([&id], |r| r.get(0))
                .map_err(internal)?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(internal)?
        };
        account::revoke_sessions(&tx, &id)?;
        tx.commit().map_err(internal)?;
        Ok(rooms)
    })
    .await?;
    disconnect(&app, rooms);
    Ok(StatusCode::NO_CONTENT)
}

async fn revoke_connection(State(app): State<App>, Path(id): Path<String>) -> Api<StatusCode> {
    let room = id.clone();
    blocking(app.clone(), move |db| {
        if db
            .execute("UPDATE connections SET revoked=1 WHERE room=?1", [id])
            .map_err(internal)?
            != 1
        {
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(())
    })
    .await?;
    disconnect(&app, vec![room]);
    Ok(StatusCode::NO_CONTENT)
}
