use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LockFile {
    pub format_version: u32,
    pub contract: ContractPin,
    pub repositories: Vec<RepoPin>,
    pub integration: IntegrationPin,
    pub reference_renderer: RendererPin,
    pub submodules: Vec<SubmodulePin>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContractPin {
    pub id: String,
    pub path: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RepoPin {
    pub id: String,
    pub path: String,
    pub sha: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntegrationPin {
    pub source_tree: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RendererPin {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SubmodulePin {
    pub id: String,
    pub path: String,
    pub sha: String,
    #[serde(default)]
    pub allowed_dev_dirt: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct LockReport {
    pub id: String,
    pub path: String,
    pub branch: String,
    pub expected_sha: String,
    pub actual_sha: String,
    pub tree: String,
    pub dirty: bool,
    pub allowed_dirt: bool,
    pub ok: bool,
    pub message: String,
}

pub fn load_lock(path: &Path) -> Result<LockFile, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn git_ref(path: &Path, git_ref: &str) -> Result<String, String> {
    let out = Command::new("git")
        .args(["rev-parse", git_ref])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn git_sha(path: &Path) -> Result<String, String> {
    git_ref(path, "HEAD")
}

pub fn git_branch(path: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok();
    out.and_then(|o| {
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "unknown".into())
}

pub fn git_tree(path: &Path) -> String {
    git_ref(path, "HEAD^{tree}").unwrap_or_default()
}

pub fn git_porcelain(path: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

pub fn git_dirty(path: &Path) -> Result<bool, String> {
    Ok(!git_porcelain(path)?.is_empty())
}

fn porcelain_paths(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.get(3..).unwrap_or(l).trim().to_string())
        .collect()
}

fn only_allowed_dirt(lines: &[String], repo_path: &str, allowed: &[String]) -> bool {
    !lines.is_empty()
        && porcelain_paths(lines).iter().all(|path| {
            allowed.iter().any(|allowed_path| {
                let rel = allowed_path
                    .strip_prefix(&format!("{repo_path}/"))
                    .unwrap_or(allowed_path);
                path == rel
            })
        })
}

pub fn git_diff_digest(path: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["diff", "--binary"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(crate::cert::sha256_bytes(&out.stdout))
}

pub fn tracked_source_tree(root: &Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    let mut files: Vec<PathBuf> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(String::from_utf8_lossy(s).as_ref()))
        .filter(|p| p != Path::new("repos.lock.toml") && !p.starts_with("release"))
        .collect();
    files.sort();
    let mut buf = Vec::new();
    for rel in files {
        buf.extend_from_slice(rel.to_string_lossy().as_bytes());
        buf.push(0);
        buf.extend(fs::read(root.join(&rel)).map_err(|e| format!("{}: {e}", rel.display()))?);
    }
    Ok(crate::cert::sha256_bytes(&buf))
}

fn report_repo(id: &str, path: &Path, expected: &str, allowed_dirt: bool) -> LockReport {
    let branch = git_branch(path);
    let actual = git_sha(path).unwrap_or_default();
    let tree = git_tree(path);
    let dirty = git_dirty(path).unwrap_or(true);
    let missing = !path.exists();
    let ok = !missing && actual == expected && (!dirty || allowed_dirt);
    let message = if missing {
        "missing repository".into()
    } else if actual != expected {
        format!("sha mismatch expected {expected} actual {actual}")
    } else if dirty && !allowed_dirt {
        "dirty worktree".into()
    } else if dirty {
        "dirty allowed in development".into()
    } else {
        "ok".into()
    };
    LockReport {
        id: id.into(),
        path: path.display().to_string(),
        branch,
        expected_sha: expected.into(),
        actual_sha: actual,
        tree,
        dirty,
        allowed_dirt,
        ok,
        message,
    }
}

pub fn verify_lock(root: &Path, lock: &LockFile, release_strict: bool) -> Vec<LockReport> {
    let mut out = Vec::new();
    let contract_path = root.join(&lock.contract.path);
    let actual = git_ref(&contract_path, &lock.contract.git_ref).unwrap_or_default();
    let ok = actual == lock.contract.sha;
    out.push(LockReport {
        id: lock.contract.id.clone(),
        path: contract_path.display().to_string(),
        branch: lock.contract.git_ref.clone(),
        expected_sha: lock.contract.sha.clone(),
        actual_sha: actual.clone(),
        tree: git_tree(&contract_path),
        dirty: false,
        allowed_dirt: false,
        ok,
        message: if ok {
            "ok".into()
        } else {
            format!("frozen ref {} is {actual}", lock.contract.git_ref)
        },
    });
    let allowed_subs: Vec<String> = lock
        .submodules
        .iter()
        .filter(|s| s.allowed_dev_dirt && !release_strict)
        .map(|s| s.path.clone())
        .collect();
    for repo in &lock.repositories {
        let path = root.join(&repo.path);
        let mut r = report_repo(&repo.id, &path, &repo.sha, false);
        if r.dirty
            && let Ok(lines) = git_porcelain(&path)
            && only_allowed_dirt(&lines, &repo.path, &allowed_subs)
        {
            r.allowed_dirt = true;
            r.ok = r.actual_sha == r.expected_sha;
            r.message = "dirty allowed in development".into();
        }
        out.push(r);
    }
    for sub in &lock.submodules {
        let allow = sub.allowed_dev_dirt && !release_strict;
        out.push(report_repo(&sub.id, &root.join(&sub.path), &sub.sha, allow));
    }
    let tree = tracked_source_tree(root).unwrap_or_default();
    let ok = tree == lock.integration.source_tree;
    out.push(LockReport {
        id: "integration_source_tree".into(),
        path: root.display().to_string(),
        branch: git_branch(root),
        expected_sha: lock.integration.source_tree.clone(),
        actual_sha: tree,
        tree: git_tree(root),
        dirty: git_dirty(root).unwrap_or(true),
        allowed_dirt: false,
        ok,
        message: if ok {
            "ok".into()
        } else {
            "source-tree digest mismatch".into()
        },
    });
    out
}

pub fn unauthorized_dirty(reports: &[LockReport]) -> bool {
    reports.iter().any(|r| r.dirty && !r.allowed_dirt)
}

pub fn format_reports(reports: &[LockReport]) -> String {
    let mut s = String::new();
    for r in reports {
        s.push_str(&format!(
            "{}: branch={} expected={} actual={} tree={} dirty={} allowed_dirt={} ok={} {}\n",
            r.id,
            r.branch,
            r.expected_sha,
            r.actual_sha,
            r.tree,
            r.dirty,
            r.allowed_dirt,
            r.ok,
            r.message
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_detection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let lock = load_lock(&root.join("repos.lock.toml")).unwrap();
        let mut lock = lock;
        lock.repositories[0].sha = "deadbeef".into();
        let reports = verify_lock(root, &lock, false);
        assert!(
            reports
                .iter()
                .any(|r| !r.ok && r.message.contains("mismatch")),
            "{reports:?}"
        );
    }

    #[test]
    fn missing_repository_detection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut lock = load_lock(&root.join("repos.lock.toml")).unwrap();
        lock.repositories.push(RepoPin {
            id: "ghost".into(),
            path: "does-not-exist".into(),
            sha: "0".into(),
        });
        let reports = verify_lock(root, &lock, false);
        assert!(
            reports
                .iter()
                .any(|r| r.id == "ghost" && r.message.contains("missing")),
            "{reports:?}"
        );
    }

    #[test]
    fn dirty_repository_rejection() {
        let tmp = std::env::temp_dir().join(format!(
            "opui-lock-dirty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(&tmp)
                .status()
                .unwrap()
                .success()
        );
        fs::write(tmp.join("a"), b"x").unwrap();
        let r = report_repo("tmp", &tmp, "missing", false);
        assert!(r.dirty);
        assert!(!r.ok);
        let _ = fs::remove_dir_all(tmp);
    }

    #[test]
    fn allowed_development_dirt() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let lock = load_lock(&root.join("repos.lock.toml")).unwrap();
        let casement = lock.submodules.iter().find(|s| s.id == "casement").unwrap();
        assert!(casement.allowed_dev_dirt);
        let allowed = vec![casement.path.clone()];
        assert!(only_allowed_dirt(
            &[" m vendor/casement".into()],
            "../openpencil",
            &allowed
        ));
        assert!(!only_allowed_dirt(
            &[" m vendor/casement".into(), " M flake.nix".into()],
            "../openpencil",
            &allowed
        ));
        assert!(!only_allowed_dirt(
            &[" M other/vendor/casement".into()],
            "../openpencil",
            &allowed
        ));
    }

    #[test]
    fn self_tree_verification() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let digest = tracked_source_tree(root).unwrap();
        assert_eq!(digest.len(), 64);
    }

    #[test]
    fn frozen_v1_branch_verification() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let lock = load_lock(&root.join("repos.lock.toml")).unwrap();
        let got = git_ref(&root.join(&lock.contract.path), &lock.contract.git_ref).unwrap();
        assert_eq!(got, lock.contract.sha);
    }
}
