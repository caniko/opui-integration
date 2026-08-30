use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cert::sha256_file;
use crate::lock::{
    self, LockFile, SourceRelation, full_sha, git_sha, load_lock, public_https_url, qualified_ref,
};

pub const SOURCE_REF: &str = "refs/heads/source/opui-v1-rc2";
pub const HOSTED_REPOSITORY: &str = "caniko/opui-integration";
const SAME_RUN_SECS: u64 = 6 * 3600;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicSourceTranscript {
    pub format_version: u32,
    pub generated_unix_time: u64,
    pub workflow: WorkflowIdentity,
    pub sources: Vec<SourceRecord>,
    pub artifacts: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BundleIndex {
    pub format_version: u32,
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowIdentity {
    pub repository: String,
    pub event_name: String,
    pub sha: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub runner_environment: String,
    pub run_id: String,
    pub run_attempt: String,
    pub workflow_ref: String,
    pub runner_os: String,
    pub runner_arch: String,
    pub image_os: String,
    pub image_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub id: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub expected_sha: String,
    pub observed_tip: String,
    pub relation: SourceRelation,
    pub observed_unix_time: u64,
    pub status: SourceStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatus {
    Pass,
    Fail,
}

pub fn generate(
    root: &Path,
    capsule: &Path,
    evidence_dirs: &[PathBuf],
    output: &Path,
) -> Result<PathBuf, String> {
    let workflow = require_hosted_dispatch(root)?;
    let lock = load_lock(&root.join("repos.lock.toml"))?;
    let output = resolve(root, output);
    require_handoff_output(root, &output)?;
    if output.exists() {
        return Err(format!(
            "public source output already exists: {}",
            output.display()
        ));
    }
    let head = git_sha(root)?;
    let now = unix_time()?;
    let mut sources = Vec::new();
    for pin in &lock.public_sources {
        sources.push(observe_pin(pin, &head, now)?);
    }
    if sources
        .iter()
        .any(|source| source.status != SourceStatus::Pass)
    {
        return Err("public source observation failed".into());
    }
    for source in &sources {
        require_fresh_timestamp(source.observed_unix_time, now)?;
    }
    let mut artifacts = BTreeMap::new();
    hash_tree(root, &resolve(root, capsule), &output, &mut artifacts)?;
    for dir in evidence_dirs {
        hash_tree(root, &resolve(root, dir), &output, &mut artifacts)?;
    }
    if artifacts.is_empty() {
        return Err("public source transcript artifacts must be nonempty".into());
    }
    let transcript = PublicSourceTranscript {
        format_version: 1,
        generated_unix_time: now,
        workflow,
        sources,
        artifacts,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&transcript).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(output)
}

pub fn validate_live(root: &Path, path: &Path) -> Result<(), String> {
    let path = resolve(root, path);
    let transcript: PublicSourceTranscript =
        serde_json::from_slice(&fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    if transcript.format_version != 1 {
        return Err("unsupported public source transcript format".into());
    }
    let live_workflow = require_hosted_dispatch(root)?;
    if transcript.workflow != live_workflow {
        return Err("transcript workflow does not match current hosted dispatch".into());
    }
    let now = unix_time()?;
    require_fresh_timestamp(transcript.generated_unix_time, now)?;
    if transcript.artifacts.is_empty() {
        return Err("public source transcript artifacts must be nonempty".into());
    }
    let lock = load_lock(&root.join("repos.lock.toml"))?;
    let head = git_sha(root)?;
    if transcript.sources.len() != lock.public_sources.len() {
        return Err("public source transcript set mismatch".into());
    }
    for (pin, recorded) in lock.public_sources.iter().zip(&transcript.sources) {
        if recorded.id != pin.id {
            return Err(format!("public source order mismatch {}", recorded.id));
        }
        require_fresh_timestamp(recorded.observed_unix_time, now)?;
        let live = observe_pin(pin, &head, recorded.observed_unix_time)?;
        if live.observed_tip != recorded.observed_tip || live.status != recorded.status {
            return Err(format!("public source {} moved or diverged", pin.id));
        }
        if live.relation != recorded.relation || live.expected_sha != recorded.expected_sha {
            return Err(format!("public source {} relation mismatch", pin.id));
        }
        if recorded.status != SourceStatus::Pass {
            return Err(format!("public source {} is not passing", pin.id));
        }
    }
    for (rel, expected) in &transcript.artifacts {
        crate::lock::full_sha256(expected)?;
        let file = confined_repo_file(root, rel)?;
        let actual = sha256_file(&file)?;
        if actual != *expected {
            return Err(format!("artifact checksum mismatch for {rel}"));
        }
    }
    Ok(())
}

pub fn write_bundle_index(
    root: &Path,
    directories: &[PathBuf],
    output: &Path,
) -> Result<PathBuf, String> {
    require_hosted_dispatch(root)?;
    let output = resolve(root, output);
    require_handoff_output(root, &output)?;
    if output.exists() {
        return Err(format!("bundle index already exists: {}", output.display()));
    }
    let mut files = BTreeMap::new();
    for directory in directories {
        hash_tree(root, &resolve(root, directory), &output, &mut files)?;
    }
    if files.is_empty() {
        return Err("bundle index must contain evidence".into());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&BundleIndex {
            format_version: 1,
            files,
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(output)
}

pub fn validate_bundle_index(root: &Path, index: &Path) -> Result<(), String> {
    let index = resolve(root, index);
    let value: BundleIndex =
        serde_json::from_slice(&fs::read(&index).map_err(|e| format!("{}: {e}", index.display()))?)
            .map_err(|e| format!("{}: {e}", index.display()))?;
    if value.format_version != 1 || value.files.is_empty() {
        return Err("invalid or empty bundle index".into());
    }
    for (path, expected) in value.files {
        crate::lock::full_sha256(&expected)?;
        let actual = sha256_file(&confined_repo_file(root, &path)?)?;
        if actual != expected {
            return Err(format!("bundle checksum mismatch for {path}"));
        }
    }
    Ok(())
}

pub fn materialize(root: &Path) -> Result<(), String> {
    let lock = load_lock(&root.join("repos.lock.toml"))?;
    for repo in &lock.repositories {
        let pin = source_by_id(&lock, &repo.id)?;
        clone_or_verify(&pin.url, &root.join(&repo.path), &repo.sha)?;
    }
    let contract = source_by_id(&lock, "opui_contract")?;
    fetch_ref(
        &root.join(&lock.contract.path),
        &contract.git_ref,
        &lock.contract.sha,
    )?;
    for sub in &lock.submodules {
        let pin = source_by_id(&lock, &sub.id)?;
        clone_or_verify(&pin.url, &root.join(&sub.path), &sub.sha)?;
    }
    Ok(())
}

fn source_by_id<'a>(lock: &'a LockFile, id: &str) -> Result<&'a lock::PublicSourcePin, String> {
    lock.public_sources
        .iter()
        .find(|pin| pin.id == id)
        .ok_or_else(|| format!("missing public source {id}"))
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn require_hosted_dispatch(root: &Path) -> Result<WorkflowIdentity, String> {
    let actions = env("GITHUB_ACTIONS");
    let runner = env("RUNNER_ENVIRONMENT");
    let repository = env("GITHUB_REPOSITORY");
    let event = env("GITHUB_EVENT_NAME");
    let sha = env("GITHUB_SHA");
    let git_ref = env("GITHUB_REF");
    let run_id = env("GITHUB_RUN_ID");
    let run_attempt = env("GITHUB_RUN_ATTEMPT");
    let workflow_ref = env("GITHUB_WORKFLOW_REF");
    let runner_os = env("RUNNER_OS");
    let runner_arch = env("RUNNER_ARCH");
    let image_os = env("ImageOS");
    let image_version = env("ImageVersion");
    let head = git_sha(root)?;
    if actions != "true"
        || runner != "github-hosted"
        || repository != HOSTED_REPOSITORY
        || event != "workflow_dispatch"
        || full_sha(&sha).is_err()
        || sha != head
        || git_ref != SOURCE_REF
        || run_id.is_empty()
        || !run_id.bytes().all(|b| b.is_ascii_digit())
        || run_attempt.is_empty()
        || !run_attempt.bytes().all(|b| b.is_ascii_digit())
        || workflow_ref.is_empty()
        || !workflow_ref.contains(HOSTED_REPOSITORY)
        || !workflow_ref.contains(SOURCE_REF)
        || runner_os.is_empty()
        || runner_arch.is_empty()
        || image_os.is_empty()
        || image_version.is_empty()
    {
        return Err("public source closure requires GitHub-hosted workflow_dispatch".into());
    }
    Ok(WorkflowIdentity {
        repository,
        event_name: event,
        sha,
        git_ref,
        runner_environment: runner,
        run_id,
        run_attempt,
        workflow_ref,
        runner_os,
        runner_arch,
        image_os,
        image_version,
    })
}

pub fn require_fresh_timestamp(ts: u64, now: u64) -> Result<(), String> {
    if ts == 0 || ts > now || now.saturating_sub(ts) > SAME_RUN_SECS {
        return Err("public source timestamp is missing, future, or stale".into());
    }
    Ok(())
}

fn observe_pin(pin: &lock::PublicSourcePin, head: &str, now: u64) -> Result<SourceRecord, String> {
    public_https_url(&pin.url)?;
    qualified_ref(&pin.git_ref)?;
    let expected = if pin.sha.is_empty() {
        full_sha(head)?.to_string()
    } else {
        full_sha(&pin.sha)?.to_string()
    };
    let observed = observe_tip(&pin.url, &pin.git_ref)?;
    let ok = match pin.relation {
        SourceRelation::Exact => observed == expected,
        SourceRelation::Ancestor => prove_ancestor(&pin.url, &pin.git_ref, &expected, &observed)?,
    };
    Ok(SourceRecord {
        id: pin.id.clone(),
        url: pin.url.clone(),
        git_ref: pin.git_ref.clone(),
        expected_sha: expected,
        observed_tip: observed,
        relation: pin.relation,
        observed_unix_time: now,
        status: if ok {
            SourceStatus::Pass
        } else {
            SourceStatus::Fail
        },
    })
}

fn observe_tip(url: &str, git_ref: &str) -> Result<String, String> {
    let output = unauthenticated_git()
        .args(["ls-remote", "--", url, git_ref])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        if name == git_ref {
            return Ok(full_sha(sha.trim())?.to_string());
        }
    }
    Err(format!("missing public ref {git_ref} at {url}"))
}

fn prove_ancestor(
    url: &str,
    git_ref: &str,
    expected: &str,
    observed: &str,
) -> Result<bool, String> {
    let branch = git_ref
        .strip_prefix("refs/heads/")
        .or_else(|| git_ref.strip_prefix("refs/tags/"))
        .ok_or_else(|| format!("unqualified ancestor ref {git_ref}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let tmp =
        std::env::temp_dir().join(format!("opui-public-source-{}-{stamp}", std::process::id()));
    if tmp.exists() {
        return Err(format!("stale ancestor clone {}", tmp.display()));
    }
    let result = (|| {
        let cloned = unauthenticated_git()
            .args([
                "clone",
                "--bare",
                "--filter=blob:none",
                "--single-branch",
                "--branch",
                branch,
                "--",
                url,
            ])
            .arg(&tmp)
            .output()
            .map_err(|e| e.to_string())?;
        if !cloned.status.success() {
            return Err(format!(
                "ancestor clone failed: {}",
                String::from_utf8_lossy(&cloned.stderr).trim()
            ));
        }
        let cloned_tip = crate::lock::git_ref(&tmp, git_ref)?;
        if cloned_tip != observed {
            return Err(format!("public source {git_ref} moved during observation"));
        }
        let proof = unauthenticated_git()
            .current_dir(&tmp)
            .args(["merge-base", "--is-ancestor", expected, observed])
            .output()
            .map_err(|e| e.to_string())?;
        match proof.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(format!(
                "ancestor proof failed: {}",
                String::from_utf8_lossy(&proof.stderr).trim()
            )),
        }
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

fn clone_or_verify(url: &str, dest: &Path, sha: &str) -> Result<(), String> {
    public_https_url(url)?;
    full_sha(sha)?;
    if dest.exists() {
        if dest.is_dir()
            && fs::read_dir(dest)
                .map_err(|e| e.to_string())?
                .next()
                .is_none()
        {
            fs::remove_dir(dest).map_err(|e| e.to_string())?;
        } else {
            let actual = git_sha(dest).unwrap_or_default();
            if actual == sha {
                return Ok(());
            }
            return Err(format!("stale destination {}", dest.display()));
        }
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let status = unauthenticated_git()
        .args(["clone", "--", url])
        .arg(dest)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("git clone failed for {url}"));
    }
    let status = unauthenticated_git()
        .current_dir(dest)
        .args(["checkout", "--detach", sha])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("git checkout failed for {sha}"));
    }
    let actual = git_sha(dest)?;
    if actual != sha {
        return Err(format!("stale destination {}", dest.display()));
    }
    Ok(())
}

fn fetch_ref(dest: &Path, git_ref: &str, sha: &str) -> Result<(), String> {
    qualified_ref(git_ref)?;
    full_sha(sha)?;
    let status = unauthenticated_git()
        .current_dir(dest)
        .args(["fetch", "origin", &format!("{git_ref}:{git_ref}")])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("git fetch failed for {git_ref}"));
    }
    let actual = crate::lock::git_ref(dest, git_ref)?;
    if actual != sha {
        return Err(format!(
            "stale destination {} {git_ref} is {actual}",
            dest.display()
        ));
    }
    Ok(())
}

fn unauthenticated_git() -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "0")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GIT_ASKPASS")
        .args(["-c", "credential.helper=", "-c", "http.extraHeader="]);
    command
}

fn hash_tree(
    root: &Path,
    dir: &Path,
    output: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    if !dir.exists() {
        return Err(format!("missing evidence directory {}", dir.display()));
    }
    walk_files(root, dir, output, out)
}

fn walk_files(
    root: &Path,
    dir: &Path,
    output: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    reject_symlink(dir)?;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path == output {
            continue;
        }
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err(format!("symlink evidence is rejected: {}", path.display()));
        }
        if meta.is_dir() {
            walk_files(root, &path, output, out)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| format!("artifact path escapes repository: {}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let file = confined_repo_file(root, &rel)?;
        out.insert(rel, sha256_file(&file)?);
    }
    Ok(())
}

pub fn confined_repo_file(root: &Path, value: &str) -> Result<PathBuf, String> {
    let rel = safe_repo_rel(value)?;
    let path = root.join(rel);
    reject_symlink(&path)?;
    let meta = fs::symlink_metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("evidence is not a regular file: {rel}"));
    }
    let root_c = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let path_c = fs::canonicalize(&path).map_err(|e| format!("{rel}: {e}"))?;
    if path_c.strip_prefix(&root_c).is_err() {
        return Err(format!("path escapes repository: {rel}"));
    }
    Ok(path)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("symlink evidence is rejected: {}", path.display()));
    }
    Ok(())
}

pub fn safe_repo_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    confined_repo_file(root, value)
}

pub fn safe_repo_rel(value: &str) -> Result<&str, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("path must be repository-relative: {value}"));
    }
    Ok(value)
}

fn require_handoff_output(root: &Path, output: &Path) -> Result<(), String> {
    let rel = output.strip_prefix(root).map_err(|_| {
        format!(
            "public source output must be under handoff/: {}",
            output.display()
        )
    })?;
    if rel.components().next().and_then(|c| c.as_os_str().to_str()) != Some("handoff") {
        return Err(format!(
            "public source output must be under handoff/: {}",
            output.display()
        ));
    }
    safe_repo_rel(&rel.to_string_lossy().replace('\\', "/"))?;
    Ok(())
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn unix_time() -> Result<u64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_rejects_non_hosted_context() {
        const CHILD: &str = "OPUI_NON_HOSTED_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "public_source::tests::generate_rejects_non_hosted_context",
                ])
                .env(CHILD, "1");
            for name in [
                "GITHUB_ACTIONS",
                "RUNNER_ENVIRONMENT",
                "GITHUB_REPOSITORY",
                "GITHUB_EVENT_NAME",
                "GITHUB_SHA",
                "GITHUB_REF",
                "GITHUB_RUN_ID",
                "GITHUB_RUN_ATTEMPT",
                "GITHUB_WORKFLOW_REF",
                "RUNNER_OS",
                "RUNNER_ARCH",
                "ImageOS",
                "ImageVersion",
            ] {
                child.env_remove(name);
            }
            assert!(child.status().unwrap().success());
            return;
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let err = generate(
            root,
            Path::new("fixtures"),
            &[],
            Path::new("handoff/public-source.json"),
        )
        .unwrap_err();
        assert!(err.contains("workflow_dispatch"), "{err}");
    }

    #[test]
    fn policy_helpers_reject_unsafe_urls_refs_and_shas() {
        assert!(public_https_url("file:///tmp/repo.git").is_err());
        assert!(public_https_url("ssh://git@github.com/caniko/opui.git").is_err());
        assert!(public_https_url("git@github.com:caniko/opui.git").is_err());
        assert!(public_https_url("https://user:token@github.com/caniko/opui.git").is_err());
        assert!(qualified_ref("trunk").is_err());
        assert!(qualified_ref("refs/heads/../trunk").is_err());
        assert!(qualified_ref("refs/heads/foo~1").is_err());
        assert!(qualified_ref("refs/heads/foo\nbar").is_err());
        assert!(qualified_ref("refs/heads/foo^*").is_err());
        assert!(full_sha("deadbeef").is_err());
        assert!(full_sha("04fdda1c8a2dabd4fad3ee66dd9043f44ed8509").is_err());
        assert!(full_sha("04FDDA1C8A2DABD4FAD3EE66DD9043F44ED8509C").is_err());
        assert!(safe_repo_rel("../secret").is_err());
        assert!(safe_repo_rel("/tmp/secret").is_err());
        assert!(safe_repo_rel("handoff/ok.json").is_ok());
    }

    #[test]
    fn timestamps_must_be_nonzero_present_and_fresh() {
        assert!(require_fresh_timestamp(0, 10).is_err());
        assert!(require_fresh_timestamp(11, 10).is_err());
        assert!(require_fresh_timestamp(1, 10 + SAME_RUN_SECS).is_err());
        require_fresh_timestamp(10, 10).unwrap();
        require_fresh_timestamp(10, 10 + SAME_RUN_SECS).unwrap();
    }

    #[test]
    fn confined_repo_file_rejects_symlinks() {
        let root = std::env::temp_dir().join(format!("opui-public-source-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ok"), b"x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", root.join("link")).unwrap();
            assert!(
                confined_repo_file(&root, "link")
                    .unwrap_err()
                    .contains("symlink")
            );
        }
        assert!(confined_repo_file(&root, "ok").is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundle_index_rejects_changed_evidence() {
        let root = std::env::temp_dir().join(format!("opui-bundle-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("handoff")).unwrap();
        fs::write(root.join("evidence"), b"before").unwrap();
        fs::write(
            root.join("handoff/index.json"),
            serde_json::to_vec(&BundleIndex {
                format_version: 1,
                files: BTreeMap::from([(
                    "evidence".into(),
                    sha256_file(&root.join("evidence")).unwrap(),
                )]),
            })
            .unwrap(),
        )
        .unwrap();
        validate_bundle_index(&root, Path::new("handoff/index.json")).unwrap();
        fs::write(root.join("evidence"), b"after").unwrap();
        assert!(
            validate_bundle_index(&root, Path::new("handoff/index.json"))
                .unwrap_err()
                .contains("checksum mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_transcript_fields_are_rejected() {
        let text = r#"{
            "format_version": 1,
            "workflow": {
                "repository": "caniko/opui-integration",
                "event_name": "workflow_dispatch",
                "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "ref": "refs/heads/source/opui-v1-rc2",
                "runner_environment": "github-hosted"
            },
            "sources": [],
            "artifacts": {},
            "unexpected": true
        }"#;
        assert!(serde_json::from_str::<PublicSourceTranscript>(text).is_err());
    }

    #[test]
    fn ancestor_proof_accepts_reachable_commit() {
        let root = std::env::temp_dir().join(format!(
            "opui-public-source-ancestor-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        for args in [
            vec!["init", "--initial-branch", "main"],
            vec!["config", "user.name", "OPUI test"],
            vec!["config", "user.email", "opui@example.invalid"],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(&root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::write(root.join("source"), b"one").unwrap();
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(["add", "source"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(["commit", "-m", "one"])
                .status()
                .unwrap()
                .success()
        );
        let expected = git_sha(&root).unwrap();
        fs::write(root.join("source"), b"two").unwrap();
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(["commit", "-am", "two"])
                .status()
                .unwrap()
                .success()
        );
        let observed = git_sha(&root).unwrap();
        assert!(
            prove_ancestor(
                root.to_str().unwrap(),
                "refs/heads/main",
                &expected,
                &observed
            )
            .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
