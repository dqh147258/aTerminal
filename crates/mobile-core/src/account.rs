use crate::{
    CoreError,
    remote::{RemoteTerminal, ffi, runtime},
};
use ai_terminal_remote::{Channel, account::AccountSession};
use std::sync::Mutex;

#[derive(uniffi::Record)]
pub struct AccountDevice {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub online: bool,
    pub current: bool,
}
/// Call account HTTP methods on the platform worker; export after each operation to persist rotation.
#[derive(uniffi::Object, Default)]
pub struct Account {
    inner: Mutex<Option<AccountSession>>,
}
#[uniffi::export]
impl Account {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn login(
        &self,
        server: String,
        username: String,
        password: String,
        name: String,
        platform: String,
        ca_pem: String,
    ) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        if state.is_some() {
            return Err(ffi("log out before switching accounts"));
        }
        let ca = if ca_pem.is_empty() {
            None
        } else {
            Some(ca_pem)
        };
        *state = Some(
            runtime()
                .block_on(AccountSession::login(
                    &server, ca, username, password, name, platform,
                ))
                .map_err(ffi)?,
        );
        Ok(())
    }
    pub fn restore(&self, value: String) -> Result<(), CoreError> {
        let session: AccountSession = serde_json::from_str(&value).map_err(ffi)?;
        ai_terminal_remote::validate_url(&session.server).map_err(ffi)?;
        *self.inner.lock().map_err(ffi)? = Some(session);
        Ok(())
    }
    pub fn export(&self) -> Result<String, CoreError> {
        let state = self.inner.lock().map_err(ffi)?;
        state
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(ffi)
            .map(|v| v.unwrap_or_default())
    }
    pub fn username(&self) -> String {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.as_ref().map(|a| a.tokens.username.clone()))
            .unwrap_or_default()
    }
    pub fn devices(&self) -> Result<Vec<AccountDevice>, CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let s = state.as_mut().ok_or_else(|| ffi("not logged in"))?;
        Ok(runtime()
            .block_on(s.devices())
            .map_err(ffi)?
            .into_iter()
            .map(|d| AccountDevice {
                id: d.id,
                name: d.name,
                platform: d.platform,
                online: d.online,
                current: d.current,
            })
            .collect())
    }
    pub fn heartbeat(&self) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let session = state.as_mut().ok_or_else(|| ffi("not logged in"))?;
        runtime().block_on(session.heartbeat()).map_err(ffi)?;
        Ok(())
    }
    pub fn connect(
        &self,
        device_id: String,
        terminal: std::sync::Arc<RemoteTerminal>,
    ) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let s = state.as_mut().ok_or_else(|| ffi("not logged in"))?;
        let channel = runtime()
            .block_on(async {
                let grant = s.connect(device_id).await?;
                Channel::account(s, &grant, true, &[]).await
            })
            .map_err(ffi)?;
        terminal.set_channel(channel)
    }
    pub fn revoke(&self, device_id: String) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let s = state.as_mut().ok_or_else(|| ffi("not logged in"))?;
        runtime().block_on(s.revoke(&device_id)).map_err(ffi)?;
        if s.tokens.device_id == device_id {
            *state = None;
        }
        Ok(())
    }
    pub fn change_password(&self, current: String, new_password: String) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let s = state.as_mut().ok_or_else(|| ffi("not logged in"))?;
        runtime()
            .block_on(s.change_password(current, new_password))
            .map_err(ffi)?;
        *state = None;
        Ok(())
    }
    pub fn logout(&self) -> Result<(), CoreError> {
        let mut state = self.inner.lock().map_err(ffi)?;
        let result = if let Some(s) = state.as_mut() {
            runtime().block_on(s.logout()).map_err(ffi)
        } else {
            Ok(())
        };
        *state = None;
        result
    }
}
