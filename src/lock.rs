use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::handoff::VERITASIUM_SHA;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LockFile {
    pub format_version: u32,
    pub contract: ContractPin,
    pub repositories: Vec<RepoPin>,
    pub integration: IntegrationPin,
    pub reference_renderer: RendererPin,
    pub submodules: Vec<SubmodulePin>,
    pub public_sources: Vec<PublicSourcePin>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceRelation {
    Exact,
    Ancestor,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicSourcePin {
    pub id: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    #[serde(default)]
    pub sha: String,
    pub relation: SourceRelation,
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
    let lock: LockFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    validate_public_source_policy(&lock)?;
    Ok(lock)
}

pub fn public_https_url(url: &str) -> Result<&str, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("public source URL must be unauthenticated HTTPS: {url}"))?;
    if rest.contains('@')
        || rest.contains("..")
        || rest.contains('\\')
        || rest.is_empty()
        || !rest.contains('/')
        || url.contains("file:")
        || url.contains("ssh:")
    {
        return Err(format!("rejected public source URL: {url}"));
    }
    Ok(url)
}

pub fn qualified_ref(git_ref: &str) -> Result<&str, String> {
    let name = git_ref
        .strip_prefix("refs/heads/")
        .or_else(|| git_ref.strip_prefix("refs/tags/"));
    let Some(name) = name else {
        return Err(format!(
            "public source ref must be fully qualified: {git_ref}"
        ));
    };
    if name.is_empty()
        || name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("//")
        || name.contains("..")
        || name.contains("@{")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'))
    {
        return Err(format!(
            "public source ref must be fully qualified: {git_ref}"
        ));
    }
    Ok(git_ref)
}

pub fn full_sha(sha: &str) -> Result<&str, String> {
    if sha.len() != 40 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(format!(
            "public source SHA must be a full 40-hex digest: {sha}"
        ));
    }
    Ok(sha)
}

pub fn full_sha256(sha: &str) -> Result<&str, String> {
    if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(format!("hash must be a full lowercase SHA-256: {sha}"));
    }
    Ok(sha)
}

struct ExpectedSource {
    id: &'static str,
    url: &'static str,
    git_ref: &'static str,
    relation: SourceRelation,
    sha: Option<String>,
}

fn repo_sha(lock: &LockFile, id: &str) -> Result<String, String> {
    lock.repositories
        .iter()
        .find(|repo| repo.id == id)
        .map(|repo| repo.sha.clone())
        .ok_or_else(|| format!("lock missing repository {id}"))
}

fn submodule_sha(lock: &LockFile, id: &str) -> Result<String, String> {
    lock.submodules
        .iter()
        .find(|sub| sub.id == id)
        .map(|sub| sub.sha.clone())
        .ok_or_else(|| format!("lock missing submodule {id}"))
}

fn expected_public_sources(lock: &LockFile) -> Result<Vec<ExpectedSource>, String> {
    Ok(vec![
        ExpectedSource {
            id: "opui",
            url: "https://github.com/caniko/opui.git",
            git_ref: "refs/heads/trunk",
            relation: SourceRelation::Exact,
            sha: Some(repo_sha(lock, "opui")?),
        },
        ExpectedSource {
            id: "opui_contract",
            url: "https://github.com/caniko/opui.git",
            git_ref: "refs/heads/feat/opui-v1-contract",
            relation: SourceRelation::Exact,
            sha: Some(lock.contract.sha.clone()),
        },
        ExpectedSource {
            id: "openpencil",
            url: "https://github.com/caniko/openpencil.git",
            git_ref: "refs/heads/source/opui-v1-rc2",
            relation: SourceRelation::Exact,
            sha: Some(repo_sha(lock, "openpencil")?),
        },
        ExpectedSource {
            id: "bevy_openpencil",
            url: "https://github.com/caniko/bevy_openpencil.git",
            git_ref: "refs/heads/source/opui-v1-rc2",
            relation: SourceRelation::Exact,
            sha: Some(repo_sha(lock, "bevy_openpencil")?),
        },
        ExpectedSource {
            id: "jian",
            url: "https://github.com/caniko/jian.git",
            git_ref: "refs/heads/source/opui-v1-rc2",
            relation: SourceRelation::Exact,
            sha: Some(submodule_sha(lock, "jian")?),
        },
        ExpectedSource {
            id: "agent",
            url: "https://github.com/ZSeven-W/agent-rs.git",
            git_ref: "refs/heads/main",
            relation: SourceRelation::Ancestor,
            sha: Some(submodule_sha(lock, "agent")?),
        },
        ExpectedSource {
            id: "casement",
            url: "https://github.com/ZSeven-W/casement.git",
            git_ref: "refs/heads/op-file-open",
            relation: SourceRelation::Exact,
            sha: Some(submodule_sha(lock, "casement")?),
        },
        ExpectedSource {
            id: "veritasium",
            url: "https://codeberg.org/caniko/rs-veritasium.git",
            git_ref: "refs/heads/trunk",
            relation: SourceRelation::Exact,
            sha: Some(VERITASIUM_SHA.into()),
        },
        ExpectedSource {
            id: "integration_trunk",
            url: "https://github.com/caniko/opui-integration.git",
            git_ref: "refs/heads/trunk",
            relation: SourceRelation::Ancestor,
            sha: Some("607c6682996313c8e3f47c46ac52c33c6be39fc6".into()),
        },
        ExpectedSource {
            id: "integration_source",
            url: "https://github.com/caniko/opui-integration.git",
            git_ref: "refs/heads/source/opui-v1-rc2",
            relation: SourceRelation::Exact,
            sha: None,
        },
    ])
}

pub fn validate_public_source_policy(lock: &LockFile) -> Result<(), String> {
    let expected = expected_public_sources(lock)?;
    let mut ids = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    for pin in &lock.public_sources {
        if !ids.insert(pin.id.as_str()) {
            return Err(format!("duplicate public source {}", pin.id));
        }
        if !pairs.insert((pin.url.as_str(), pin.git_ref.as_str())) {
            return Err(format!(
                "duplicate public source ref {} {}",
                pin.url, pin.git_ref
            ));
        }
        public_https_url(&pin.url)?;
        qualified_ref(&pin.git_ref)?;
        if !pin.sha.is_empty() {
            full_sha(&pin.sha)?;
        }
    }
    if lock.public_sources.len() != expected.len() {
        return Err("public source set mismatch".into());
    }
    for exp in &expected {
        let pin = lock
            .public_sources
            .iter()
            .find(|pin| pin.id == exp.id)
            .ok_or_else(|| format!("missing public source {}", exp.id))?;
        if pin.url != exp.url || pin.git_ref != exp.git_ref || pin.relation != exp.relation {
            return Err(format!("public source {} policy mismatch", exp.id));
        }
        match &exp.sha {
            None if !pin.sha.is_empty() => {
                return Err(format!(
                    "public source {} must not pin a source SHA",
                    exp.id
                ));
            }
            Some(sha) if pin.sha != *sha => {
                return Err(format!("public source {} sha mismatch", exp.id));
            }
            _ => {}
        }
    }
    Ok(())
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

    #[test]
    fn public_source_policy_rejects_missing_extra_and_duplicate() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let lock = load_lock(&root.join("repos.lock.toml")).unwrap();
        validate_public_source_policy(&lock).unwrap();

        let mut missing = lock.clone();
        missing.public_sources.pop();
        assert!(
            validate_public_source_policy(&missing)
                .unwrap_err()
                .contains("mismatch")
        );

        let mut extra = lock.clone();
        let mut pin = extra.public_sources[0].clone();
        pin.id = "ghost".into();
        pin.git_ref = "refs/heads/ghost".into();
        extra.public_sources.push(pin);
        assert!(
            validate_public_source_policy(&extra)
                .unwrap_err()
                .contains("mismatch")
        );

        let mut duplicate = lock.clone();
        duplicate
            .public_sources
            .push(duplicate.public_sources[0].clone());
        assert!(
            validate_public_source_policy(&duplicate)
                .unwrap_err()
                .contains("duplicate")
        );

        let mut drifted = lock.clone();
        drifted.public_sources[0].sha = "0".repeat(40);
        assert!(
            validate_public_source_policy(&drifted)
                .unwrap_err()
                .contains("sha mismatch")
        );

        let mut cycled = lock.clone();
        let integration = cycled
            .public_sources
            .iter_mut()
            .find(|pin| pin.id == "integration_source")
            .unwrap();
        integration.sha = "1".repeat(40);
        assert!(
            validate_public_source_policy(&cycled)
                .unwrap_err()
                .contains("must not pin")
        );
    }
}
