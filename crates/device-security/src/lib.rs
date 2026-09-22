pub mod account;
// Authenticated Noise channel: pinned desktop public key and per-pair capability.
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
const PATTERN: &str = "Noise_NKpsk0_25519_ChaChaPoly_BLAKE2s";
const MAX_MESSAGE: usize = 4 * 1024 * 1024 + 4096;
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub struct Invitation {
    pub version: u32,
    pub server: String,
    pub room: String,
    pub relay_token: String,
    pub desktop_public: String,
    pub pairing_key: String,
    #[serde(default)]
    pub ice_servers: Vec<String>,
    /// Explicit trust bootstrap supplied by the desktop, never fetched from the relay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_ca_pem: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct HostIdentity {
    pub private: String,
    pub public: String,
    pub pairing_key: String,
}
impl HostIdentity {
    pub fn generate() -> Result<Self> {
        let keypair = snow::Builder::new(PATTERN.parse()?).generate_keypair()?;
        Ok(Self {
            private: encode(&keypair.private),
            public: encode(&keypair.public),
            pairing_key: random_secret()?,
        })
    }
    pub fn responder(&self) -> Result<snow::HandshakeState> {
        let private = key(&self.private)?;
        let psk = key(&self.pairing_key)?;
        Ok(snow::Builder::new(PATTERN.parse()?)
            .prologue(b"ai-terminal/channel/2")?
            .local_private_key(&private)?
            .psk(0, &psk)?
            .build_responder()?)
    }
}
impl Invitation {
    pub fn initiator(&self) -> Result<snow::HandshakeState> {
        if self.version != 1 {
            bail!("unsupported pairing version")
        }
        let public = key(&self.desktop_public)?;
        let psk = key(&self.pairing_key)?;
        Ok(snow::Builder::new(PATTERN.parse()?)
            .prologue(b"ai-terminal/channel/2")?
            .remote_public_key(&public)?
            .psk(0, &psk)?
            .build_initiator()?)
    }
    pub fn export(&self) -> Result<String> {
        Ok(format!("aiterminal:{}", encode(&serde_json::to_vec(self)?)))
    }
    pub fn import(s: &str) -> Result<Self> {
        if s.len() > 16_384 {
            bail!("oversized invitation")
        }
        let bytes = URL_SAFE_NO_PAD.decode(
            s.trim()
                .strip_prefix("aiterminal:")
                .context("invalid invitation")?,
        )?;
        let value: Self = serde_json::from_slice(&bytes)?;
        value.initiator()?;
        Ok(value)
    }
}
pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn random_secret() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("randomness unavailable: {e}"))?;
    Ok(encode(&bytes))
}
fn key(s: &str) -> Result<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(s)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid 32-byte key"))
}

pub struct Cipher {
    state: snow::TransportState,
    pending: Vec<u8>,
    expected: Option<usize>,
}
impl Cipher {
    pub fn new(handshake: snow::HandshakeState) -> Result<Self> {
        Ok(Self {
            state: handshake.into_transport_mode()?,
            pending: Vec::new(),
            expected: None,
        })
    }
    pub fn encrypt(&mut self, message: &[u8]) -> Result<Vec<Vec<u8>>> {
        if message.len() > MAX_MESSAGE {
            bail!("oversized encrypted message")
        }
        let mut wire = Vec::with_capacity(message.len() + 4);
        wire.extend_from_slice(&(message.len() as u32).to_be_bytes());
        wire.extend_from_slice(message);
        let mut records = Vec::new();
        for chunk in wire.chunks(CHUNK) {
            let mut record = vec![0; chunk.len() + 16];
            let n = self.state.write_message(chunk, &mut record)?;
            record.truncate(n);
            records.push(record)
        }
        Ok(records)
    }
    pub fn decrypt(&mut self, record: &[u8]) -> Result<Option<Vec<u8>>> {
        if record.len() > CHUNK + 16 {
            bail!("oversized encrypted record")
        }
        let mut plain = vec![0; record.len()];
        let n = self.state.read_message(record, &mut plain)?;
        plain.truncate(n);
        if self.pending.len() + plain.len() > MAX_MESSAGE + 4 {
            bail!("oversized reassembly")
        }
        self.pending.extend_from_slice(&plain);
        if self.expected.is_none() && self.pending.len() >= 4 {
            let len = u32::from_be_bytes(self.pending[..4].try_into().unwrap()) as usize;
            if len > MAX_MESSAGE {
                bail!("oversized decrypted message")
            };
            self.expected = Some(len + 4);
        }
        if let Some(len) = self.expected {
            if self.pending.len() > len {
                bail!("invalid encrypted framing")
            }
            if self.pending.len() == len {
                let data = self.pending.split_off(4);
                self.pending.clear();
                self.expected = None;
                return Ok(Some(data));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn handshake() -> (Cipher, Cipher) {
        let identity = HostIdentity::generate().unwrap();
        let invite = Invitation {
            version: 1,
            server: "".into(),
            room: "".into(),
            relay_token: "".into(),
            desktop_public: identity.public.clone(),
            pairing_key: identity.pairing_key.clone(),
            ice_servers: Vec::new(),
            server_ca_pem: None,
        };
        let mut a = invite.initiator().unwrap();
        let mut b = identity.responder().unwrap();
        let mut wire = [0; 256];
        let mut plain = [0; 256];
        let n = a.write_message(&[], &mut wire).unwrap();
        b.read_message(&wire[..n], &mut plain).unwrap();
        let n = b.write_message(&[], &mut wire).unwrap();
        a.read_message(&wire[..n], &mut plain).unwrap();
        (Cipher::new(a).unwrap(), Cipher::new(b).unwrap())
    }
    #[test]
    fn fragmented_messages_round_trip_and_replay_is_rejected() {
        let (mut a, mut b) = handshake();
        let data = vec![137; 100_000];
        let frames = a.encrypt(&data).unwrap();
        let mut result = None;
        for f in &frames {
            result = b.decrypt(f).unwrap().or(result)
        }
        assert_eq!(result.unwrap(), data);
        assert!(b.decrypt(&frames[0]).is_err());
    }
    #[test]
    fn wrong_pairing_capability_cannot_authenticate() {
        let identity = HostIdentity::generate().unwrap();
        let invite = Invitation {
            version: 1,
            server: "".into(),
            room: "".into(),
            relay_token: "".into(),
            desktop_public: identity.public.clone(),
            pairing_key: random_secret().unwrap(),
            ice_servers: Vec::new(),
            server_ca_pem: None,
        };
        let mut a = invite.initiator().unwrap();
        let mut b = identity.responder().unwrap();
        let mut wire = [0; 256];
        let n = a.write_message(&[], &mut wire).unwrap();
        assert!(b.read_message(&wire[..n], &mut [0; 256]).is_err());
    }
}
