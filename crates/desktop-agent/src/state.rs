//! Default root discovery and explicit, resumable migration. Never kill a daemon.
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub fn home_root() -> Result<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .context("home directory unavailable; specify --state-dir")?;
    let home = PathBuf::from(home);
    ensure!(home.is_absolute(), "home directory must be absolute");
    Ok(home.join(".aTerminal"))
}
pub fn legacy_root() -> PathBuf {
    #[cfg(unix)]
    {
        std::env::temp_dir().join(format!(
            "ai-terminal-{}",
            rustix::process::getuid().as_raw()
        ))
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join("ai-terminal")
    }
}
pub fn default_root() -> Result<PathBuf> {
    let target = home_root()?;
    let legacy = legacy_root();
    if legacy.exists() && crate::Client::connect(&legacy).is_ok() {
        eprintln!(
            "Using the running legacy Desktop; migrate after its sessions are closed: aTerminal config migrate --from {}",
            legacy.display()
        );
        return Ok(legacy);
    }
    if target.join("migration.pending.json").exists() {
        anyhow::bail!("migration incomplete; resume config migrate --from the original directory");
    }
    if legacy.join("agent.lock").exists() {
        let lock = crate::service::open_private(&legacy.join("agent.lock"), false)?;
        ensure!(
            lock.try_lock_exclusive().is_ok(),
            "legacy Desktop is starting or unreachable; do not start a second default instance"
        );
    }
    if !target.join("config.json").exists()
        && (legacy.join("account-mode").exists() || legacy.join("pairs").exists())
    {
        anyhow::bail!(
            "legacy Desktop data found; run aTerminal config migrate --from {}",
            legacy.display()
        );
    }
    Ok(target)
}
#[derive(Serialize, Deserialize)]
struct Migration {
    source: PathBuf,
    vault_id: String,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct VaultReference {
    pub id: String,
    pub legacy_file: Option<PathBuf>,
}
/// `from` remains an intact backup. Only persistent identity/pair files are copied.
pub fn migrate(from: &Path, target: &Path) -> Result<()> {
    ensure!(from.is_dir(), "migration_source_not_found");
    ensure!(
        !from.join("config.json").exists(),
        "source is already versioned; legacy migration will not discard its configuration"
    );
    crate::service::secure_dir(from)?;
    crate::service::secure_dir(target)?;
    let vault_id = blake3::hash(from.to_string_lossy().as_bytes())
        .to_hex()
        .to_string();
    let from = from.canonicalize()?;
    let target = target.canonicalize()?;
    ensure!(from != target, "migration_same_directory");
    ensure!(
        crate::Client::connect(&from).is_err(),
        "legacy_daemon_running"
    );
    let source_lock = crate::service::open_private(&from.join("agent.lock"), false)?;
    source_lock
        .try_lock_exclusive()
        .context("legacy daemon owns source directory")?;
    ensure!(
        crate::Client::connect(&target).is_err(),
        "target_daemon_running"
    );
    let target_lock = crate::service::open_private(&target.join("agent.lock"), false)?;
    target_lock
        .try_lock_exclusive()
        .context("target daemon owns directory")?;
    let pending = target.join("migration.pending.json");
    let migration = if pending.exists() {
        let m: Migration = serde_json::from_slice(&std::fs::read(&pending)?)?;
        ensure!(m.source == from, "migration_source_conflict");
        m
    } else {
        ensure!(
            !target.join("config.json").exists()
                && !target.join("account-mode").exists()
                && !target.join("pairs").exists(),
            "migration_target_conflict"
        );
        let m = Migration {
            source: from.clone(),
            vault_id,
        };
        write_json(&pending, &m)?;
        m
    };
    if from.join("account-mode").exists() {
        copy_private(
            &from.join("account-mode"),
            &target.join("account-mode"),
            false,
        )?;
    }
    if from.join("pairs").exists() {
        crate::service::secure_dir(&from.join("pairs"))?;
        crate::service::secure_dir(&target.join("pairs"))?;
        for (n, entry) in std::fs::read_dir(from.join("pairs"))?.enumerate() {
            ensure!(n < 1024, "migration_pair_limit");
            let entry = entry?;
            ensure!(
                entry.path().extension().is_some_and(|s| s == "json"),
                "unknown_legacy_pair_file"
            );
            copy_private(
                &entry.path(),
                &target.join("pairs").join(entry.file_name()),
                true,
            )?;
        }
    }
    let credentials = target.join("credentials");
    crate::service::secure_dir(&credentials)?;
    let old_root = std::env::var_os("XDG_CONFIG_HOME")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".config")))
        .unwrap_or_else(|| from.clone());
    let old_file = old_root
        .join("ai-terminal")
        .join(format!("{}.json", migration.vault_id));
    if old_file.exists() {
        copy_private(&old_file, &credentials.join("account.json"), true)?;
    }
    write_json(
        &credentials.join("account-reference.json"),
        &VaultReference {
            id: migration.vault_id,
            legacy_file: None,
        },
    )?;
    let _ = crate::config::ConfigService::open(&target)?;
    std::fs::rename(pending, target.join("migration.complete.json"))?;
    Ok(())
}
fn copy_private(source: &Path, target: &Path, require_private: bool) -> Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 1024 * 1024,
        "invalid_migration_source"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            meta.uid() == rustix::process::getuid().as_raw()
                && (!require_private || meta.mode() & 0o077 == 0),
            "insecure_migration_source"
        );
    }
    #[cfg(not(unix))]
    let _ = require_private;
    let bytes = std::fs::read(source)?;
    if target.exists() {
        ensure!(std::fs::read(target)? == bytes, "migration_file_conflict");
        return Ok(());
    }
    let mut file = crate::service::open_private(target, false)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension("tmp");
    let mut file = crate::service::open_private(&temp, false)?;
    file.set_len(0)?;
    serde_json::to_writer(&mut file, value)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    #[cfg(unix)]
    std::fs::File::open(path.parent().context("missing parent")?)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_identity_and_source_and_rejects_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("legacy");
        let target = d.path().join("new");
        crate::service::secure_dir(&source).unwrap();
        // Legacy versions used fs::write, which created this non-secret marker as 0644.
        std::fs::write(source.join("account-mode"), b"account").unwrap();
        let expected = blake3::hash(source.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        migrate(&source, &target).unwrap();
        assert!(source.join("account-mode").exists());
        assert!(target.join("migration.complete.json").exists());
        assert!(!target.join("endpoint.json").exists());
        let reference: VaultReference = serde_json::from_slice(
            &std::fs::read(target.join("credentials/account-reference.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(reference.id, expected);
        assert!(migrate(&source, &target).is_err());
    }
    #[test]
    fn locked_legacy_is_never_copied() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("legacy");
        crate::service::secure_dir(&source).unwrap();
        let lock = crate::service::open_private(&source.join("agent.lock"), false).unwrap();
        lock.lock_exclusive().unwrap();
        assert!(migrate(&source, &d.path().join("new")).is_err());
        assert!(!d.path().join("new/migration.pending.json").exists());
    }
}
