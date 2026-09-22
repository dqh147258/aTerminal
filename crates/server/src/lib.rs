pub mod account;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{delete, get, post},
};
use futures_util::{SinkExt, StreamExt};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct App {
    db: Arc<Mutex<Connection>>,
    admin: [u8; 32],
    rooms: Arc<Mutex<HashMap<String, Slots>>>,
    logins: Arc<Mutex<HashMap<String, (i64, u32)>>>,
    password_slots: Arc<tokio::sync::Semaphore>,
    dummy_hash: String,
}
type Sender = mpsc::Sender<Message>;
#[derive(Default)]
struct Slots {
    desktop: Option<(u64, Sender)>,
    mobile: Option<(u64, Sender)>,
}
#[derive(Serialize)]
struct Pair {
    room: String,
    desktop_token: String,
    mobile_token: String,
    expires_at: i64,
}

pub fn router(path: &std::path::Path, token: &str) -> Result<Router> {
    anyhow::ensure!(
        token.len() >= 32,
        "admin token must contain at least 32 characters"
    );
    let db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS pairs (room TEXT PRIMARY KEY, desktop BLOB NOT NULL, mobile BLOB NOT NULL, expires_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0);")?;
    account::migrate(&db)?;
    let app = App {
        db: Arc::new(Mutex::new(db)),
        admin: *blake3::hash(token.as_bytes()).as_bytes(),
        rooms: Arc::default(),
        logins: Arc::default(),
        password_slots: Arc::new(tokio::sync::Semaphore::new(2)),
        dummy_hash: account::dummy_hash()?,
    };
    let router = Router::new()
        .merge(account::routes())
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/pairs", post(create_pair))
        .route("/v1/pairs/{room}", delete(revoke_pair))
        .route("/v1/relay/{room}/{role}", get(relay))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(app);
    Ok(router)
}
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
fn admin(app: &App, headers: &HeaderMap) -> bool {
    bearer(headers).is_some_and(|v| blake3::hash(v.as_bytes()).as_bytes() == &app.admin)
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
async fn create_pair(State(app): State<App>, headers: HeaderMap) -> Result<Json<Pair>, StatusCode> {
    if !admin(&app, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pair = Pair {
        room: ai_terminal_security::random_secret()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        desktop_token: ai_terminal_security::random_secret()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        mobile_token: ai_terminal_security::random_secret()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        expires_at: now() + 30 * 24 * 3600,
    };
    let db = app.db.clone();
    let room = pair.room.clone();
    let desktop = *blake3::hash(pair.desktop_token.as_bytes()).as_bytes();
    let mobile = *blake3::hash(pair.mobile_token.as_bytes()).as_bytes();
    let expiry = pair.expires_at;
    tokio::task::spawn_blocking(move || {
        db.lock().unwrap().execute(
            "INSERT INTO pairs(room,desktop,mobile,expires_at) VALUES (?1,?2,?3,?4)",
            params![room, desktop, mobile, expiry],
        )
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(pair))
}
async fn revoke_pair(
    State(app): State<App>,
    Path(room): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !admin(&app, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let db = app.db.clone();
    let id = room.clone();
    tokio::task::spawn_blocking(move || {
        db.lock()
            .unwrap()
            .execute("UPDATE pairs SET revoked=1 WHERE room=?1", [id])
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(slots) = app.rooms.lock().unwrap().remove(&room) {
        for (_, sender) in slots.desktop.into_iter().chain(slots.mobile) {
            let _ = sender.try_send(Message::Close(None));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn relay(
    State(app): State<App>,
    Path((room, role)): Path<(String, String)>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if role != "desktop" && role != "mobile" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let token = bearer(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    let hash = *blake3::hash(token.as_bytes()).as_bytes();
    let db = app.db.clone();
    let id = room.clone();
    let desktop = role == "desktop";
    let expiry = tokio::task::spawn_blocking(move || {
        let db = db.lock().unwrap();
        let sql = if desktop {
            "SELECT expires_at FROM pairs WHERE room=?1 AND desktop=?2 AND revoked=0"
        } else {
            "SELECT expires_at FROM pairs WHERE room=?1 AND mobile=?2 AND revoked=0"
        };
        db.query_row(sql, params![id, hash], |r| r.get::<_, i64>(0))
            .ok()
            .filter(|expiry| *expiry > now())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::UNAUTHORIZED)?;
    {
        let rooms = app.rooms.lock().unwrap();
        if rooms.len() >= 128 && !rooms.contains_key(&room) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    }
    Ok(ws
        .max_message_size(32768)
        .max_frame_size(32768)
        .on_upgrade(move |socket| connection(app, room, desktop, expiry, socket, false)))
}
async fn connection(
    app: App,
    room: String,
    desktop: bool,
    expiry: i64,
    socket: WebSocket,
    account_room: bool,
) {
    let id = getrandom::u64().unwrap_or(0);
    let (tx, mut rx) = mpsc::channel::<Message>(8);
    {
        // Serialize registration with revocation so a delayed upgrade cannot revive a pair.
        let db = app.db.lock().unwrap();
        let allowed = if account_room {
            account::allowed(&db, &room)
        } else {
            db.query_row(
                "SELECT expires_at FROM pairs WHERE room=?1 AND revoked=0",
                [&room],
                |r| r.get::<_, i64>(0),
            )
            .is_ok_and(|e| e > now())
        };
        if !allowed {
            return;
        }
        let mut rooms = app.rooms.lock().unwrap();
        let slots = rooms.entry(room.clone()).or_default();
        let own = if desktop {
            &mut slots.desktop
        } else {
            &mut slots.mobile
        };
        if let Some((_, old)) = own.replace((id, tx.clone())) {
            let _ = old.try_send(Message::Close(None));
            let other = if desktop {
                &mut slots.mobile
            } else {
                &mut slots.desktop
            };
            if let Some((_, peer)) = other.take() {
                let _ = peer.try_send(Message::Close(None));
            }
        }
        if let (Some((_, a)), Some((_, b))) = (&slots.desktop, &slots.mobile) {
            let _ = a.try_send(Message::Text("ready".into()));
            let _ = b.try_send(Message::Text("ready".into()));
        }
    }
    drop(tx);
    let (mut sink, mut source) = socket.split();
    let expiration = tokio::time::sleep(std::time::Duration::from_secs(
        expiry.saturating_sub(now()).max(0) as u64,
    ));
    tokio::pin!(expiration);
    let mut authorization = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _=authorization.tick(), if account_room => {
                let allowed = account::allowed(&app.db.lock().unwrap(), &room);
                if !allowed { break; }
            },
            _=&mut expiration=>break,
            outgoing=rx.recv()=>match outgoing{Some(message)=>{let closed=matches!(message,Message::Close(_));if !matches!(tokio::time::timeout(std::time::Duration::from_secs(5),sink.send(message)).await,Ok(Ok(())))||closed{break}},None=>break},
            incoming=source.next()=>match incoming{
                Some(Ok(Message::Binary(bytes)))=>{
                    let peer={let rooms=app.rooms.lock().unwrap();rooms.get(&room).and_then(|s|{
                        let own=if desktop{&s.desktop}else{&s.mobile};
                        if own.as_ref().is_none_or(|(current,_)|*current!=id){return None}
                        if desktop{s.mobile.as_ref()}else{s.desktop.as_ref()}
                    }).map(|(_,tx)|tx.clone())};
                    if let Some(peer)=peer{if !matches!(tokio::time::timeout(std::time::Duration::from_secs(5),peer.send(Message::Binary(bytes))).await,Ok(Ok(()))){break}}else{break}
                },
                Some(Ok(Message::Ping(bytes)))=>{if sink.send(Message::Pong(bytes)).await.is_err(){break}},
                Some(Ok(Message::Pong(_)))=>{},_=>break,
            }
        }
    }
    let _ = sink.close().await;
    let mut rooms = app.rooms.lock().unwrap();
    if let Some(slots) = rooms.get_mut(&room) {
        let own = if desktop {
            &mut slots.desktop
        } else {
            &mut slots.mobile
        };
        if own.as_ref().is_some_and(|(current, _)| *current == id) {
            *own = None;
            let other = if desktop {
                &mut slots.mobile
            } else {
                &mut slots.desktop
            };
            if let Some((_, peer)) = other.take() {
                let _ = peer.try_send(Message::Close(None));
            }
        }
        if slots.desktop.is_none() && slots.mobile.is_none() {
            rooms.remove(&room);
        }
    }
    drop(rooms);
    if account_room {
        let _ = app
            .db
            .lock()
            .unwrap()
            .execute("UPDATE connections SET revoked=1 WHERE room=?1", [&room]);
    }
}
