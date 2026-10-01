//! Desktop-owned MRU. Only validated creation paths and OS process observations enter here.
use anyhow::{Context, Result, ensure};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub(crate) const LIMIT: usize = 20;
const MAX_PATH_BYTES: usize = 16384;

pub(crate) fn validate(path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("working directory is unavailable")?;
    ensure!(path.is_dir(), "working directory is not a directory");
    Ok(path)
}

pub(crate) struct RecentDirectories {
    root: PathBuf,
}
impl RecentDirectories {
    pub(crate) fn new(root: &Path) -> Self {
        Self {
            root: root.join("data/recent-directories"),
        }
    }
    fn file(&self, owner: &str) -> PathBuf {
        self.root
            .join(format!("{}.json", blake3::hash(owner.as_bytes()).to_hex()))
    }
    pub(crate) fn list(&self, owner: &str) -> Result<Vec<String>> {
        let file = match std::fs::File::open(self.file(owner)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take((LIMIT * MAX_PATH_BYTES * 6 + 1024) as u64)
            .read_to_end(&mut bytes)?;
        let paths: Vec<String> = serde_json::from_slice(&bytes)?;
        let mut result = Vec::new();
        for path in paths {
            if !path.is_empty()
                && path.len() <= MAX_PATH_BYTES
                && Path::new(&path).is_absolute()
                && !path.contains('\0')
                && !result.contains(&path)
            {
                result.push(path);
                if result.len() == LIMIT {
                    break;
                }
            }
        }
        Ok(result)
    }
    pub(crate) fn record(&mut self, owner: &str, path: &Path) -> Result<()> {
        let path = validate(path)?;
        let path = path.to_str().context("working directory is not UTF-8")?;
        ensure!(
            path.len() <= MAX_PATH_BYTES,
            "working directory is too long"
        );
        let mut paths = self.list(owner).unwrap_or_default();
        if paths.first().is_some_and(|first| first == path) {
            return Ok(());
        }
        paths.retain(|entry| entry != path);
        paths.insert(0, path.into());
        paths.truncate(LIMIT);
        crate::service::secure_dir(&self.root)?;
        crate::state::write_json(&self.file(owner), &paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_mru_persists_and_isolates_owners() {
        let root = tempfile::tempdir().unwrap();
        let mut recent = RecentDirectories::new(root.path());
        let mut dirs = Vec::new();
        for i in 0..25 {
            let path = root
                .path()
                .join(format!("directory {i}; $(echo untouched)"));
            std::fs::create_dir(&path).unwrap();
            recent.record("alice", &path).unwrap();
            dirs.push(path.canonicalize().unwrap().to_str().unwrap().to_string());
        }
        recent.record("alice", Path::new(&dirs[10])).unwrap();
        recent.record("alice", Path::new(&dirs[10])).unwrap();
        let reopened = RecentDirectories::new(root.path());
        let list = reopened.list("alice").unwrap();
        assert_eq!(list.len(), LIMIT);
        assert_eq!(list[0], dirs[10]);
        assert_eq!(list[1], dirs[24]);
        assert!(!list.contains(&dirs[0]));
        assert!(reopened.list("bob").unwrap().is_empty());
        assert!(reopened.list("").unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(reopened.file("alice"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn invalid_paths_never_enter_history_and_symlinks_deduplicate() {
        let root = tempfile::tempdir().unwrap();
        let mut recent = RecentDirectories::new(root.path());
        let file = root.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(recent.record("a", &file).is_err());
        assert!(recent.record("a", &root.path().join("missing")).is_err());
        recent.record("a", root.path()).unwrap();
        #[cfg(unix)]
        {
            let link = root.path().join("link");
            std::os::unix::fs::symlink(root.path(), &link).unwrap();
            recent.record("a", &link).unwrap();
        }
        assert_eq!(recent.list("a").unwrap().len(), 1);
    }
}
