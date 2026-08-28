use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(hex(Sha256::digest(bytes)))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

pub fn sidecar_digest(root: &Path) -> Result<String, String> {
    if !root.exists() {
        return Ok(sha256_bytes(b""));
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for (rel, path) in files {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(path).map_err(|e| e.to_string())?);
    }
    Ok(hex(hasher.finalize()))
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    for ent in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let ent = ent.map_err(|e| e.to_string())?;
        let path = ent.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
    Ok(())
}

pub fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn new_run_dir(root: &Path, case: &str, size: &str) -> Result<PathBuf, String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let dir = run_root(root).join(format!("{case}-{size}-{ts}"));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub fn run_root(root: &Path) -> PathBuf {
    std::env::var_os("OPUI_RUN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("runs"))
}

/// Refuse to reuse a package unless its hash matches `expected`.
pub fn install_package(src: &Path, dest: &Path, expected: Option<&str>) -> Result<String, String> {
    let hash = sha256_file(src)?;
    if dest.exists() {
        let existing = sha256_file(dest)?;
        match expected {
            Some(exp) if existing == exp && hash == exp => return Ok(hash),
            Some(exp) => {
                return Err(format!(
                    "stale package {} hash {existing} != expected {exp}",
                    dest.display()
                ));
            }
            None if existing != hash => {
                fs::remove_file(dest).map_err(|e| e.to_string())?;
                let assets = dest.with_file_name(format!(
                    "{}.assets",
                    dest.file_name().unwrap().to_string_lossy()
                ));
                if assets.exists() {
                    fs::remove_dir_all(&assets).map_err(|e| e.to_string())?;
                }
            }
            None => {}
        }
    }
    if let Some(exp) = expected
        && hash != exp
    {
        return Err(format!("source hash {hash} != expected {exp}"));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(src, dest).map_err(|e| e.to_string())?;
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stale_package_is_rejected_or_replaced() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("opui-stale-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        let src = dir.join("fresh.opui");
        let dest = dir.join("out.opui");
        fs::write(&src, b"fresh-bytes").unwrap();
        fs::write(&dest, b"stale-bytes").unwrap();
        let expected = sha256_bytes(b"fresh-bytes");
        let err = install_package(&src, &dest, Some(&expected)).unwrap_err();
        assert!(err.contains("stale"), "{err}");
        install_package(&src, &dest, None).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"fresh-bytes");
        let _ = fs::remove_dir_all(dir);
    }
}
