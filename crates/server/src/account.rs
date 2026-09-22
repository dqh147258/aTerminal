//! User/device/session authorization. All password work runs off the async executor.
use super::*;
use ai_terminal_security::account::*;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rusqlite::OptionalExtension;
use std::{path::Path as FilePath, time::Duration};

type Api<T> = std::result::Result<T, StatusCode>;
fn internal(_: impl std::fmt::Display) -> StatusCode {
    StatusCode::INTERNAL_SERVER_ERROR
}
fn secret() -> Api<String> {
    ai_terminal_security::random_secret().map_err(internal)
}
fn digest(value: &str) -> Vec<u8> {
    blake3::hash(value.as_bytes()).as_bytes().to_vec()
}
pub fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch("PRAGMA foreign_keys=ON;
    CREATE TABLE IF NOT EXISTS users(id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), name TEXT NOT NULL, platform TEXT NOT NULL, public_key TEXT NOT NULL UNIQUE, last_seen INTEGER NOT NULL DEFAULT 0, revoked INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS auth_sessions(id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id), access BLOB NOT NULL UNIQUE, refresh BLOB NOT NULL UNIQUE, access_expiry INTEGER NOT NULL, refresh_expiry INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS connections(room TEXT PRIMARY KEY, desktop_id TEXT NOT NULL REFERENCES devices(id), mobile_id TEXT NOT NULL REFERENCES devices(id), grant_json TEXT NOT NULL, expires_at INTEGER NOT NULL, lease_expiry INTEGER NOT NULL, desktop_used INTEGER NOT NULL DEFAULT 0, mobile_used INTEGER NOT NULL DEFAULT 0, revoked INTEGER NOT NULL DEFAULT 0);
    CREATE INDEX IF NOT EXISTS device_user ON devices(user_id);
    CREATE INDEX IF NOT EXISTS session_device ON auth_sessions(device_id);")?;
    Ok(())
}
fn username(value: &str) -> Api<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-@".contains(&b))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}
fn password_hash(password: &str) -> Result<String> {
    anyhow::ensure!(
        (12..=1024).contains(&password.len()),
        "password must contain 12 to 1024 bytes"
    );
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| anyhow::anyhow!("{e}"))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| anyhow::anyhow!("{e}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|p| p.to_string())
        .map_err(|e| anyhow::anyhow!("{e}"))
}
fn verify(password: &str, hash: &str) -> bool {
    password.len() <= 1024
        && PasswordHash::new(hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
}
pub fn manage_user(path: &FilePath, name: &str, password: &str, reset: bool) -> Result<()> {
    username(name).map_err(|_| anyhow::anyhow!("invalid username"))?;
    let hash = password_hash(password)?;
    let mut db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(2))?;
    migrate(&db)?;
    let tx = db.transaction()?;
    if reset {
        anyhow::ensure!(
            tx.execute(
                "UPDATE users SET password=?1 WHERE username=?2",
                params![hash, name]
            )? == 1,
            "user not found"
        );
        tx.execute("DELETE FROM auth_sessions WHERE device_id IN (SELECT d.id FROM devices d JOIN users u ON u.id=d.user_id WHERE u.username=?1)",[name])?;
        tx.execute("UPDATE connections SET revoked=1 WHERE desktop_id IN (SELECT d.id FROM devices d JOIN users u ON u.id=d.user_id WHERE u.username=?1) OR mobile_id IN (SELECT d.id FROM devices d JOIN users u ON u.id=d.user_id WHERE u.username=?1)",[name])?;
    } else {
        tx.execute(
            "INSERT INTO users VALUES (?1,?2,?3)",
            params![ai_terminal_security::random_secret()?, name, hash],
        )?;
    }
    tx.commit()?;
    Ok(())
}
struct Principal {
    user: String,
    device: String,
    platform: String,
}
fn authenticate(db: &Connection, headers: &HeaderMap) -> Api<Principal> {
    let token = bearer(headers)
        .filter(|v| v.len() <= 128)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    db.query_row("SELECT d.user_id,d.id,u.username,d.platform FROM auth_sessions s JOIN devices d ON d.id=s.device_id JOIN users u ON u.id=d.user_id WHERE s.access=?1 AND s.access_expiry>?2 AND s.refresh_expiry>?2 AND d.revoked=0",params![digest(token),now()],|r| Ok(Principal {user:r.get(0)?,device:r.get(1)?,platform:r.get(3)?})).map_err(|_|StatusCode::UNAUTHORIZED)
}
async fn blocking<T: Send + 'static>(
    app: App,
    f: impl FnOnce(&mut Connection) -> Api<T> + Send + 'static,
) -> Api<T> {
    tokio::task::spawn_blocking(move || f(&mut *app.db.lock().map_err(internal)?))
        .await
        .map_err(internal)?
}
fn issue(db: &Connection, device: &str, name: &str) -> Api<Tokens> {
    let value = Tokens {
        access_token: secret()?,
        refresh_token: secret()?,
        expires_at: now() + 900,
        device_id: device.into(),
        username: name.into(),
    };
    db.execute(
        "INSERT INTO auth_sessions VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            secret()?,
            device,
            digest(&value.access_token),
            digest(&value.refresh_token),
            value.expires_at,
            now() + 30 * 86400
        ],
    )
    .map_err(internal)?;
    Ok(value)
}
pub(super) fn routes() -> Router<App> {
    Router::new()
        .route("/v2/auth/login", post(login))
        .route("/v2/auth/refresh", post(refresh))
        .route("/v2/auth/logout", post(logout))
        .route("/v2/auth/password", post(password))
        .route("/v2/devices", get(devices))
        .route("/v2/devices/{id}", delete(revoke))
        .route("/v2/devices/heartbeat", post(heartbeat))
        .route("/v2/connections", post(connect))
        .route("/v2/relay/{room}/{role}", get(relay_account))
}
async fn login(State(app): State<App>, Json(req): Json<LoginRequest>) -> Api<Json<Tokens>> {
    username(&req.username)?;
    if req.password.len() > 1024
        || req.device_name.is_empty()
        || req.device_name.len() > 128
        || !["desktop", "ios", "android"].contains(&req.platform.as_str())
        || validate_public(&req.public_key).is_err()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    {
        let mut attempts = app.logins.lock().map_err(internal)?;
        let current = now();
        attempts.retain(|_, (t, _)| current - *t < 60);
        if attempts.len() >= 1024 && !attempts.contains_key(&req.username) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let entry = attempts.entry(req.username.clone()).or_insert((current, 0));
        if entry.1 >= 5 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        entry.1 += 1;
    }
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    // Read the hash under the DB lock, verify outside it, then recheck it in the transaction.
    let name = req.username.clone();
    let row = blocking(app.clone(), move |db| {
        db.query_row(
            "SELECT id,password FROM users WHERE username=?1",
            [name],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(internal)
    })
    .await?;
    let fallback = app.dummy_hash.clone();
    let password = req.password;
    let expected = row.as_ref().map_or(fallback, |r| r.1.clone());
    let checked = expected.clone();
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify(&password, &checked)
    })
    .await
    .map_err(internal)?;
    let Some((user, stored)) = row.filter(|_| valid) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    blocking(app, move |db| {
        let tx = db.transaction().map_err(internal)?;
        if !tx
            .query_row("SELECT password FROM users WHERE id=?1", [&user], |r| {
                r.get::<_, String>(0)
            })
            .is_ok_and(|v| v == stored)
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        // A public key is never silently transferred between accounts, platforms or revoked devices.
        let existing = tx
            .query_row(
                "SELECT id,user_id,platform,revoked FROM devices WHERE public_key=?1",
                [&req.public_key],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(internal)?;
        let device = if let Some((id, owner, platform, revoked)) = existing {
            if owner != user || platform != req.platform || revoked != 0 {
                return Err(StatusCode::CONFLICT);
            }
            id
        } else {
            let id = secret()?;
            tx.execute(
                "INSERT INTO devices(id,user_id,name,platform,public_key) VALUES (?1,?2,?3,?4,?5)",
                params![id, user, req.device_name, req.platform, req.public_key],
            )
            .map_err(internal)?;
            id
        };
        tx.execute("DELETE FROM auth_sessions WHERE device_id=?1", [&device])
            .map_err(internal)?;
        tx.execute(
            "UPDATE connections SET revoked=1 WHERE desktop_id=?1 OR mobile_id=?1",
            [&device],
        )
        .map_err(internal)?;
        let tokens = issue(&tx, &device, &req.username)?;
        tx.commit().map_err(internal)?;
        Ok(Json(tokens))
    })
    .await
}
async fn refresh(State(app): State<App>, Json(req): Json<RefreshRequest>) -> Api<Json<Tokens>> {
    if req.refresh_token.len() > 128 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    blocking(app,move|db|{let tx=db.transaction().map_err(internal)?;
        let (id,device,name)=tx.query_row("SELECT s.id,d.id,u.username FROM auth_sessions s JOIN devices d ON d.id=s.device_id JOIN users u ON u.id=d.user_id WHERE s.refresh=?1 AND s.refresh_expiry>?2 AND d.revoked=0",params![digest(&req.refresh_token),now()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|_|StatusCode::UNAUTHORIZED)?;
        tx.execute("DELETE FROM auth_sessions WHERE id=?1",[id]).map_err(internal)?;
        let tokens=issue(&tx,&device,&name)?;tx.commit().map_err(internal)?;Ok(Json(tokens))
    }).await
}
async fn logout(State(app): State<App>, headers: HeaderMap) -> Api<StatusCode> {
    blocking(app, move |db| {
        let tx = db.transaction().map_err(internal)?;
        let p = authenticate(&tx, &headers)?;
        revoke_sessions(&tx, &p.device)?;
        tx.commit().map_err(internal)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
fn revoke_sessions(db: &Connection, id: &str) -> Api<()> {
    db.execute("DELETE FROM auth_sessions WHERE device_id=?1", [id])
        .map_err(internal)?;
    db.execute(
        "UPDATE connections SET revoked=1 WHERE desktop_id=?1 OR mobile_id=?1",
        [id],
    )
    .map_err(internal)?;
    db.execute("UPDATE devices SET last_seen=0 WHERE id=?1", [id])
        .map_err(internal)?;
    Ok(())
}
async fn password(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<PasswordRequest>,
) -> Api<StatusCode> {
    if !(12..=1024).contains(&req.new_password.len()) || req.current_password.len() > 1024 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let permit = app
        .password_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let h = headers.clone();
    let (user, old) = blocking(app.clone(), move |db| {
        let p = authenticate(db, &h)?;
        let hash = db
            .query_row("SELECT password FROM users WHERE id=?1", [&p.user], |r| {
                r.get::<_, String>(0)
            })
            .map_err(internal)?;
        Ok((p.user, hash))
    })
    .await?;
    let expected = old.clone();
    let new = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if !verify(&req.current_password, &expected) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        password_hash(&req.new_password).map_err(internal)
    })
    .await
    .map_err(internal)??;
    blocking(app,move|db|{let tx=db.transaction().map_err(internal)?;authenticate(&tx,&headers)?;
        if tx.execute("UPDATE users SET password=?1 WHERE id=?2 AND password=?3",params![new,user,old]).map_err(internal)?!=1{return Err(StatusCode::UNAUTHORIZED)}
        tx.execute("DELETE FROM auth_sessions WHERE device_id IN (SELECT id FROM devices WHERE user_id=?1)",[&user]).map_err(internal)?;
        tx.execute("UPDATE connections SET revoked=1 WHERE desktop_id IN (SELECT id FROM devices WHERE user_id=?1) OR mobile_id IN (SELECT id FROM devices WHERE user_id=?1)",[&user]).map_err(internal)?;
        tx.commit().map_err(internal)?;Ok(StatusCode::NO_CONTENT)
    }).await
}
async fn devices(State(app): State<App>, headers: HeaderMap) -> Api<Json<Vec<Device>>> {
    blocking(app,move|db|{let p=authenticate(db,&headers)?;let mut stmt=db.prepare("SELECT id,name,platform,public_key,last_seen FROM devices WHERE user_id=?1 AND revoked=0 ORDER BY name,id").map_err(internal)?;
        let rows=stmt.query_map([p.user],|r|{let id=r.get::<_,String>(0)?;Ok(Device {current:id==p.device,id,name:r.get(1)?,platform:r.get(2)?,public_key:r.get(3)?,online:r.get::<_,i64>(4)?>now()-15})}).map_err(internal)?;
        Ok(Json(rows.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?))}).await
}
async fn revoke(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Api<StatusCode> {
    blocking(app, move |db| {
        let tx = db.transaction().map_err(internal)?;
        let p = authenticate(&tx, &headers)?;
        if tx
            .execute(
                "UPDATE devices SET revoked=1 WHERE id=?1 AND user_id=?2",
                params![id, p.user],
            )
            .map_err(internal)?
            != 1
        {
            return Err(StatusCode::NOT_FOUND);
        }
        revoke_sessions(&tx, &id)?;
        tx.commit().map_err(internal)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
async fn heartbeat(State(app): State<App>, headers: HeaderMap) -> Api<Json<Vec<ConnectionGrant>>> {
    blocking(app,move|db|{let p=authenticate(db,&headers)?;db.execute("UPDATE devices SET last_seen=?1 WHERE id=?2",params![now(),p.device]).map_err(internal)?;
        db.execute("DELETE FROM connections WHERE lease_expiry<?1 OR (expires_at<?1 AND desktop_used=0)",[now()]).map_err(internal)?;
        let mut stmt=db.prepare("SELECT grant_json FROM connections WHERE desktop_id=?1 AND desktop_used=0 AND expires_at>?2 AND revoked=0 LIMIT 16").map_err(internal)?;
        let json=stmt.query_map(params![p.device,now()],|r|r.get::<_,String>(0)).map_err(internal)?.collect::<rusqlite::Result<Vec<_>>>().map_err(internal)?;
        Ok(Json(json.iter().map(|s|serde_json::from_str(s).map_err(internal)).collect::<Api<Vec<_>>>()?))}).await
}
async fn connect(
    State(app): State<App>,
    headers: HeaderMap,
    Json(req): Json<ConnectRequest>,
) -> Api<Json<ConnectionGrant>> {
    blocking(app,move|db|{let tx=db.transaction().map_err(internal)?;let p=authenticate(&tx,&headers)?;
        if p.platform=="desktop" {return Err(StatusCode::FORBIDDEN)}
        let desktop_public=tx.query_row("SELECT public_key FROM devices WHERE id=?1 AND user_id=?2 AND platform='desktop' AND revoked=0 AND last_seen>?3",params![req.desktop_id,p.user,now()-15],|r|r.get::<_,String>(0)).map_err(|_|StatusCode::NOT_FOUND)?;
        let mobile_public=tx.query_row("SELECT public_key FROM devices WHERE id=?1",[&p.device],|r|r.get::<_,String>(0)).map_err(internal)?;
        let count:i64=tx.query_row("SELECT count(*) FROM connections WHERE (desktop_id=?1 OR mobile_id=?2) AND revoked=0 AND lease_expiry>?3",params![req.desktop_id,p.device,now()],|r|r.get(0)).map_err(internal)?;
        if count>=16 {return Err(StatusCode::TOO_MANY_REQUESTS)}
        let grant=ConnectionGrant {version:2,room:secret()?,desktop_id:req.desktop_id,mobile_id:p.device,desktop_public,mobile_public,expires_at:now()+60,read_only:false};
        tx.execute("INSERT INTO connections(room,desktop_id,mobile_id,grant_json,expires_at,lease_expiry) VALUES (?1,?2,?3,?4,?5,?6)",params![grant.room,grant.desktop_id,grant.mobile_id,serde_json::to_string(&grant).map_err(internal)?,grant.expires_at,now()+12*3600]).map_err(internal)?;
        tx.commit().map_err(internal)?;Ok(Json(grant))}).await
}
async fn relay_account(
    State(app): State<App>,
    Path((room, role)): Path<(String, String)>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Api<Response> {
    if role != "desktop" && role != "mobile" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let desktop = role == "desktop";
    let id = room.clone();
    let expiry=blocking(app.clone(),move|db|{let tx=db.transaction().map_err(internal)?;let p=authenticate(&tx,&headers)?;
        let sql=if desktop {"UPDATE connections SET desktop_used=1 WHERE room=?1 AND desktop_id=?2 AND desktop_used=0 AND revoked=0 AND expires_at>?3 RETURNING lease_expiry"} else {"UPDATE connections SET mobile_used=1 WHERE room=?1 AND mobile_id=?2 AND mobile_used=0 AND revoked=0 AND expires_at>?3 RETURNING lease_expiry"};
        let expiry=tx.query_row(sql,params![id,p.device,now()],|r|r.get::<_,i64>(0)).map_err(|_|StatusCode::UNAUTHORIZED)?;tx.commit().map_err(internal)?;Ok(expiry)}).await?;
    {
        let rooms = app.rooms.lock().map_err(internal)?;
        if rooms.len() >= 128 && !rooms.contains_key(&room) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    }
    Ok(ws
        .max_message_size(32768)
        .max_frame_size(32768)
        .on_upgrade(move |socket| connection(app, room, desktop, expiry, socket, true)))
}
pub(super) fn allowed(db: &Connection, room: &str) -> bool {
    db.query_row("SELECT c.lease_expiry FROM connections c JOIN devices d ON d.id=c.desktop_id JOIN devices m ON m.id=c.mobile_id WHERE c.room=?1 AND c.revoked=0 AND d.revoked=0 AND m.revoked=0 AND EXISTS(SELECT 1 FROM auth_sessions WHERE device_id=d.id AND refresh_expiry>?2) AND EXISTS(SELECT 1 FROM auth_sessions WHERE device_id=m.id AND refresh_expiry>?2)",params![room,now()],|r|r.get::<_,i64>(0)).is_ok_and(|e|e>now())
}
pub(super) fn dummy_hash() -> Result<String> {
    password_hash(&ai_terminal_security::random_secret()?)
}
