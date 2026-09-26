//! Credentials use the same OS-vault/file-backend policy as account identity.
use anyhow::{Result, ensure};
use std::{io::Write, path::Path};
fn file_backend() -> bool {
    cfg!(unix) && std::env::var("AI_TERMINAL_CREDENTIAL_STORE").is_ok_and(|v| v == "file")
        || cfg!(target_os = "linux") && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
}
fn identity(root: &Path, owner: &str, reference: &str) -> Result<String> {
    ensure!(
        reference.len() == 32 && reference.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid_secret_reference"
    );
    Ok(
        blake3::hash(serde_json::to_string(&(root, owner, reference))?.as_bytes())
            .to_hex()
            .to_string(),
    )
}
pub(crate) fn put(root: &Path, owner: &str, secret: &str) -> Result<String> {
    ensure!(
        !secret.is_empty() && secret.len() <= 16384,
        "invalid_secret"
    );
    let reference = format!("{:016x}{:016x}", crate::random_id(), crate::random_id());
    let id = identity(root, owner, &reference)?;
    if file_backend() {
        let directory = root.join("credentials");
        crate::service::secure_dir(&directory)?;
        let mut file =
            crate::service::open_private(&directory.join(format!("provider-{id}")), false)?;
        file.write_all(secret.as_bytes())?;
        file.sync_all()?;
    } else {
        keyring::Entry::new("dev.aiterminal.providers", &id)?.set_password(secret)?;
    }
    Ok(reference)
}
pub(crate) fn get(root: &Path, owner: &str, reference: &str) -> Result<String> {
    let id = identity(root, owner, reference)?;
    if file_backend() {
        let file = root.join("credentials").join(format!("provider-{id}"));
        let metadata = std::fs::symlink_metadata(&file)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 16384,
            "invalid_secret_file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.mode() & 0o077 == 0
                    && metadata.uid() == rustix::process::getuid().as_raw(),
                "insecure_secret_file"
            );
        }
        Ok(std::fs::read_to_string(file)?)
    } else {
        Ok(keyring::Entry::new("dev.aiterminal.providers", &id)?.get_password()?)
    }
}
pub(crate) fn remove(root: &Path, owner: &str, reference: &str) -> Result<()> {
    let id = identity(root, owner, reference)?;
    if file_backend() {
        std::fs::remove_file(root.join("credentials").join(format!("provider-{id}")))?;
    } else {
        keyring::Entry::new("dev.aiterminal.providers", &id)?.delete_credential()?;
    }
    Ok(())
}
