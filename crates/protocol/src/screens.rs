//! Read-only Desktop screen contract. JSON lives in Reply.history[0].
use serde::{Deserialize, Serialize};

pub const SCREEN_PROTOCOL_VERSION: u32 = 1;
pub const MAX_SCREEN_WIDTH: u32 = 1920;
pub const MAX_SCREEN_JPEG_BYTES: usize = 120 * 1024;
// 120 KiB JPEG => 160 KiB base64. Reserve >80 KiB for protobuf/envelope,
// encryption, other in-flight traffic under transport::peer's 256 KiB budget.
pub const MAX_SCREEN_JSON_BYTES: usize = 164 * 1024;
pub const MAX_SCREENS: usize = 64;
pub const MAX_SCREEN_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScreenInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenList {
    pub screens: Vec<ScreenInfo>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenFrame {
    pub screen_id: String,
    pub mime_type: String,
    pub image_base64: String,
    pub width: u32,
    pub height: u32,
    pub captured_at_ms: u64,
}

pub fn valid_frame_request(screen_id: &str, max_width: u32) -> bool {
    !screen_id.is_empty()
        && screen_id.len() <= MAX_SCREEN_ID_BYTES
        && !screen_id.chars().any(char::is_control)
        && (1..=MAX_SCREEN_WIDTH).contains(&max_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MAX_MESSAGE_BYTES,
        local::{Operation, Reply, Request},
    };
    use prost::Message;
    #[test]
    fn screen_requests_roundtrip_without_terminal_identity_and_reserve_transport_budget() {
        let request = Request {
            operation: Operation::RemoteScreenFrame as i32,
            screen_id: "macos:1".into(),
            screen_max_width: 1200,
            ..Default::default()
        };
        assert_eq!(
            Request::decode(request.encode_to_vec().as_slice()).unwrap(),
            request
        );
        assert!(request.session.is_empty());
        let frame = ScreenFrame {
            screen_id: request.screen_id,
            mime_type: "image/jpeg".into(),
            image_base64: "A".repeat(MAX_SCREEN_JPEG_BYTES.div_ceil(3) * 4),
            width: 1920,
            height: 1080,
            captured_at_ms: 1,
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.len() < MAX_SCREEN_JSON_BYTES);
        let reply = Reply {
            history: vec![json],
            ..Default::default()
        };
        assert!(reply.encoded_len() + 4096 < 256 * 1024);
        assert!(reply.encoded_len() < MAX_MESSAGE_BYTES);
        assert!(!valid_frame_request("", 1));
        assert!(!valid_frame_request("id", 0));
        assert!(!valid_frame_request("id", MAX_SCREEN_WIDTH + 1));
    }
}
