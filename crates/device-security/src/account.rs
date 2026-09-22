//! Account control-plane records. Private device keys never cross the server API.
use crate::{encode, key};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub private: String,
    pub public: String,
}
impl DeviceIdentity {
    pub fn generate() -> Result<Self> {
        let pair =
            snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?).generate_keypair()?;
        Ok(Self {
            private: encode(&pair.private),
            public: encode(&pair.public),
        })
    }
    pub fn handshake(
        &self,
        grant: &ConnectionGrant,
        initiator: bool,
    ) -> Result<snow::HandshakeState> {
        let local = key(&self.private)?;
        let remote = key(if initiator {
            &grant.desktop_public
        } else {
            &grant.mobile_public
        })?;
        ensure!(
            self.public
                == if initiator {
                    &grant.mobile_public
                } else {
                    &grant.desktop_public
                }
                .as_str(),
            "device identity does not match grant"
        );
        let prologue = serde_json::to_vec(grant)?;
        let builder = snow::Builder::new("Noise_IK_25519_ChaChaPoly_BLAKE2s".parse()?)
            .prologue(&prologue)?
            .local_private_key(&local)?;
        if initiator {
            Ok(builder.remote_public_key(&remote)?.build_initiator()?)
        } else {
            Ok(builder.build_responder()?)
        }
    }
}
pub fn validate_public(value: &str) -> Result<()> {
    key(value).map(|_| ())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub device_name: String,
    pub platform: String,
    pub public_key: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub device_id: String,
    pub username: String,
}
#[derive(Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}
#[derive(Serialize, Deserialize)]
pub struct PasswordRequest {
    pub current_password: String,
    pub new_password: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub public_key: String,
    pub online: bool,
    pub current: bool,
}
#[derive(Serialize, Deserialize)]
pub struct ConnectRequest {
    pub desktop_id: String,
}
/// Canonical serialized grant is also the Noise handshake prologue. The directory is trusted.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionGrant {
    pub version: u32,
    pub room: String,
    pub desktop_id: String,
    pub mobile_id: String,
    pub desktop_public: String,
    pub mobile_public: String,
    pub expires_at: i64,
    pub read_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutual_identity_and_grant_binding() {
        let desktop = DeviceIdentity::generate().unwrap();
        let mobile = DeviceIdentity::generate().unwrap();
        let grant = ConnectionGrant {
            version: 2,
            room: "nonce".into(),
            desktop_id: "d".into(),
            mobile_id: "m".into(),
            desktop_public: desktop.public.clone(),
            mobile_public: mobile.public.clone(),
            expires_at: 123,
            read_only: false,
        };
        let mut a = mobile.handshake(&grant, true).unwrap();
        let mut b = desktop.handshake(&grant, false).unwrap();
        let mut wire = [0; 256];
        let mut plain = [0; 256];
        let n = a.write_message(&[], &mut wire).unwrap();
        b.read_message(&wire[..n], &mut plain).unwrap();
        assert_eq!(b.get_remote_static().unwrap(), key(&mobile.public).unwrap());
        let n = b.write_message(&[], &mut wire).unwrap();
        a.read_message(&wire[..n], &mut plain).unwrap();
        assert!(a.is_handshake_finished() && b.is_handshake_finished());
        let mut changed = grant.clone();
        changed.read_only = true;
        let mut a = mobile.handshake(&grant, true).unwrap();
        let mut b = desktop.handshake(&changed, false).unwrap();
        let n = a.write_message(&[], &mut wire).unwrap();
        assert!(b.read_message(&wire[..n], &mut plain).is_err());
        assert!(
            DeviceIdentity::generate()
                .unwrap()
                .handshake(&grant, true)
                .is_err()
        );
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum DesktopAccountCommand {
    Login {
        server: String,
        ca: Option<String>,
        username: String,
        password: String,
        device_name: String,
    },
    Status,
    Logout,
    Devices,
    Revoke {
        device_id: String,
    },
}
