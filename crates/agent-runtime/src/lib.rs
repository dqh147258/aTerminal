//! Desktop agent model boundary. Terminal authorization and durable state belong to Host.
mod builtin;
pub mod catalog;
pub mod config;
pub mod history;
pub mod host;
pub mod mcp;
pub mod model;
pub mod store;

/// Client request nonce; retry the same nonce after uncertain transport delivery.
pub fn request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}
pub mod extensions;
