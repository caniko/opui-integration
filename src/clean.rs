use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cert::sha256_file;
use crate::lock::load_lock;

#[derive(Debug)]
pub struct CleanReleaseResult {
    pub evidence: PathBuf,
    pub aggregate_success: bool,
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn certify_release_clean(root: &Path, profile: &str) -> Result<CleanReleaseResult, String> {
    let first = certify_release_clean_once(root, profile)?;
    if profile != "rc1" || !first.aggregate_success {
        return Ok(first);
    }
    let second = certify_release_clean_once(root, profile)?;
    let first_manifest = first.evidence.join("artifact-manifest.json");
    let second_manifest = second.evidence.join("artifact-manifest.json");
    let first_hash = sha256_file(&first_manifest)?;
    let second_hash = sha256_file(&second_manifest)?;
    let matching = first_hash == second_hash;
    fs::write(
        second.evidence.join("paired-clean.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "first_run": first.evidence,
            "second_run": second.evidence,
            "first_artifact_manifest_sha256": first_hash,
            "second_artifact_manifest_sha256": second_hash,
            "matching_artifact_hashes": matching,
            "status": if matching { "pass" } else { "fail" },
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(CleanReleaseResult {
        evidence: second.evidence,
        aggregate_success: second.aggregate_success && matching,
    })
}

fn certify_release_clean_once(root: &Path, profile: &str) -> Result<CleanReleaseResult, String> {
    let lock = load_lock(&root.join("repos.lock.toml"))?;
    let temp = TempRoot(unique_temp_root()?);
    let work = temp.0.join("work");
    fs::create_dir(&work).map_err(|e| e.to_string())?;

    for repo in &lock.repositories {
        clone_exact(&root.join(&repo.path), &work.join(&repo.id), &repo.sha)?;
    }
    let integration_sha = git(root, ["rev-parse", "HEAD"])?;
    let clean_root = work.join("opui-integration");
    clone_exact(root, &clean_root, &integration_sha)?;

    let opui = work.join("opui");
    git(
        &opui,
        [
            "update-ref",
            &format!("refs/heads/{}", lock.contract.git_ref),
            &lock.contract.sha,
        ],
    )?;

    for submodule in &lock.submodules {
        let source = root.join(&submodule.path);
        let destination = clean_root.join(&submodule.path);
        remove_empty_submodule_placeholder(&destination)?;
        clone_exact(&source, &destination, &submodule.sha)?;
        validate_repo(&destination, &submodule.sha)?;
    }
    let openpencil_pin = lock
        .repositories
        .iter()
        .find(|repo| repo.id == "openpencil")
        .ok_or("lock has no OpenPencil repository")?;
    validate_repo(&work.join("openpencil"), &openpencil_pin.sha)?;

    let target = temp.0.join("target");
    let cargo_home = temp.0.join("cargo-home");
    let run_root = temp.0.join("runs");
    let xdg_cache = temp.0.join("xdg-cache");
    fs::create_dir(&target).map_err(|e| e.to_string())?;
    fs::create_dir(&cargo_home).map_err(|e| e.to_string())?;
    fs::create_dir(&run_root).map_err(|e| e.to_string())?;
    fs::create_dir(&xdg_cache).map_err(|e| e.to_string())?;

    let openpencil = work.join("openpencil");
    let nix_roots = temp.0.join("nix-roots");
    fs::create_dir(&nix_roots).map_err(|e| e.to_string())?;
    let renderer_output = nix_build(
        &openpencil,
        "reference-renderer",
        &nix_roots.join("reference-renderer"),
    )?;
    let raster_output = nix_build(
        &openpencil,
        "op-cli-raster",
        &nix_roots.join("op-cli-raster"),
    )?;
    let renderer = renderer_output.join("bin/op-reference-renderer");
    let raster = raster_output.join("bin/op");
    let renderer_hash = sha256_file(&renderer)?;
    let raster_hash = sha256_file(&raster)?;
    if renderer_hash != lock.reference_renderer.sha256 {
        return Err(format!(
            "clean renderer hash {renderer_hash} != locked {}",
            lock.reference_renderer.sha256
        ));
    }
    if !raster.is_file() {
        return Err(format!(
            "missing clean raster exporter {}",
            raster.display()
        ));
    }
    let pinned_renderer = clean_root.join(&lock.reference_renderer.path);
    fs::create_dir_all(
        pinned_renderer
            .parent()
            .ok_or("locked renderer has no parent")?,
    )
    .map_err(|e| e.to_string())?;
    fs::copy(&renderer, &pinned_renderer).map_err(|e| e.to_string())?;

    for repo in [
        &clean_root,
        &work.join("bevy_openpencil"),
        &work.join("opui"),
        &openpencil,
    ] {
        let mut command = cargo_command("cargo", &target, &cargo_home, &run_root, &raster);
        command.args(["fetch", "--locked"]).current_dir(repo);
        require_success(command, "cargo fetch")?;
    }

    let mut strict = cargo_command("cargo", &target, &cargo_home, &run_root, &raster);
    strict
        .args([
            "run",
            "--quiet",
            "--bin",
            "opui-certify",
            "--",
            "--verify-lock-strict",
        ])
        .current_dir(&clean_root);
    require_success(strict, "strict repository lock")?;

    let mut aggregate = cargo_command("cargo", &target, &cargo_home, &run_root, &raster);
    aggregate
        .args([
            "run",
            "--quiet",
            "--bin",
            "opui-certify",
            "--",
            "--release-profile",
            profile,
        ])
        .current_dir(&clean_root);
    let aggregate = run(aggregate, "release aggregate")?;

    let release = only_release_dir(&run_root)?;
    fs::write(release.join("clean-aggregate.stdout"), &aggregate.stdout)
        .map_err(|e| e.to_string())?;
    fs::write(release.join("clean-aggregate.stderr"), &aggregate.stderr)
        .map_err(|e| e.to_string())?;
    let aggregate_success = aggregate.status.success();
    fs::write(
        release.join("capsule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "integration": integration_sha,
            "profile": profile,
            "repositories": lock.repositories,
            "submodules": lock.submodules,
            "reference_renderer": {
                "nix_output": renderer_output,
                "sha256": renderer_hash,
            },
            "raster_exporter": raster_output,
            "raster_exporter_sha256": raster_hash,
            "fresh_cargo_home": true,
            "fresh_target": true,
            "aggregate_success": aggregate_success,
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let stable = root.join("release");
    let destination = stable.join(release.file_name().ok_or("release has no name")?);
    rewrite_json_paths(&release, &release, &destination)?;
    let evidence = if release.join("complete").is_file() {
        promote_completed_bundle(&release, &stable)?
    } else {
        retain_failed_bundle(&release, &stable)?
    };
    refresh_provenance_manifest_hash(&evidence)?;
    Ok(CleanReleaseResult {
        evidence,
        aggregate_success,
    })
}

fn refresh_provenance_manifest_hash(evidence: &Path) -> Result<(), String> {
    let manifest = evidence.join("artifact-manifest.json");
    let provenance = evidence.join("provenance.json");
    if !manifest.is_file() || !provenance.is_file() {
        return Ok(());
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&provenance).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    value["artifact_manifest_sha256"] = sha256_file(&manifest)?.into();
    fs::write(
        provenance,
        serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn cargo_command(
    program: &str,
    target: &Path,
    cargo_home: &Path,
    run_root: &Path,
    raster: &Path,
) -> Command {
    let mut command = Command::new(program);
    command
        .env("CARGO_TARGET_DIR", target)
        .env("CARGO_HOME", cargo_home)
        .env("OPUI_RUN_ROOT", run_root)
        .env("OPUI_RASTER_EXPORTER", raster)
        .env("OPENPENCIL_OP", raster)
        .env("NIX_USER_CONF_FILES", "/dev/null")
        .env_remove("CARGO_PROFILE_DEV_CODEGEN_BACKEND")
        .env_remove("LIBRARY_PATH")
        .env_remove("VK_ICD_FILENAMES")
        .env("XDG_CACHE_HOME", run_root.with_file_name("xdg-cache"));
    command
}

fn unique_temp_root() -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("opui-release-{}-{stamp}", std::process::id()));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn clone_exact(source: &Path, destination: &Path, sha: &str) -> Result<(), String> {
    if destination.exists() {
        return Err(format!("stale clone destination {}", destination.display()));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut clone = Command::new("git");
    clone
        .args(["clone", "--no-local", "--no-checkout"])
        .arg(source)
        .arg(destination);
    require_success(clone, "git clone")?;
    git(destination, ["checkout", "--detach", sha])?;
    validate_repo(destination, sha)
}

fn remove_empty_submodule_placeholder(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir()
        || fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err(format!("stale clone destination {}", path.display()));
    }
    fs::remove_dir(path).map_err(|e| e.to_string())
}

fn validate_repo(path: &Path, expected: &str) -> Result<(), String> {
    if !path.join(".git").exists() {
        return Err(format!("missing repository {}", path.display()));
    }
    let actual = git(path, ["rev-parse", "HEAD"])?;
    if actual != expected {
        return Err(format!(
            "{} is {actual}, expected {expected}",
            path.display()
        ));
    }
    let status = git(path, ["status", "--porcelain"])?;
    if !status.is_empty() {
        return Err(format!("dirty repository {}: {status}", path.display()));
    }
    Ok(())
}

fn nix_build(root: &Path, package: &str, out_link: &Path) -> Result<PathBuf, String> {
    let mut command = Command::new("nix");
    command
        .args(["build", "--print-out-paths", "--out-link"])
        .arg(out_link)
        .arg(format!("path:{}#{package}", root.display()))
        .env("NIX_USER_CONF_FILES", "/dev/null")
        .env_remove("CARGO_HOME")
        .env_remove("LIBRARY_PATH")
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("CARGO_PROFILE_DEV_CODEGEN_BACKEND");
    let output = run(command, &format!("nix build {package}"))?;
    if !output.status.success() {
        return Err(format_output(&format!("nix build {package}"), &output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .map(|line| PathBuf::from(line.trim()))
        .ok_or_else(|| format!("nix build {package} returned no output path"))
}

fn only_release_dir(root: &Path) -> Result<PathBuf, String> {
    let mut releases = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .is_some_and(|name| name.starts_with("release-"))
        });
    let release = releases
        .next()
        .ok_or("release aggregate produced no bundle")?;
    if releases.next().is_some() {
        return Err("release aggregate produced multiple bundles".into());
    }
    Ok(release)
}

pub(crate) fn promote_completed_bundle(
    source: &Path,
    stable_root: &Path,
) -> Result<PathBuf, String> {
    for required in ["complete", "release-report.json", "release-report.md"] {
        if !source.join(required).is_file() {
            return Err(format!("incomplete release bundle: missing {required}"));
        }
    }
    copy_bundle(source, stable_root)
}

fn retain_failed_bundle(source: &Path, stable_root: &Path) -> Result<PathBuf, String> {
    if source.join("complete").is_file() {
        return Err("refusing to retain completed bundle as failed".into());
    }
    copy_bundle(source, stable_root)
}

fn copy_bundle(source: &Path, stable_root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(stable_root).map_err(|e| e.to_string())?;
    let name = source.file_name().ok_or("release bundle has no name")?;
    let destination = stable_root.join(name);
    if destination.exists() {
        return Err(format!("stale release output {}", destination.display()));
    }
    let stage = stable_root.join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    if stage.exists() {
        return Err(format!("stale release stage {}", stage.display()));
    }
    copy_dir(source, &stage)?;
    fs::rename(&stage, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn rewrite_json_paths(root: &Path, from: &Path, to: &Path) -> Result<(), String> {
    fn rewrite(value: &mut serde_json::Value, from: &str, to: &str) {
        match value {
            serde_json::Value::String(value) if value.starts_with(from) => {
                *value = value.replacen(from, to, 1);
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    rewrite(value, from, to);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rewrite(value, from, to);
                }
            }
            _ => {}
        }
    }

    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            rewrite_json_paths(&path, from, to)?;
        } else if path.extension() == Some(OsStr::new("json")) {
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            rewrite(&mut value, &from.to_string_lossy(), &to.to_string_lossy());
            fs::write(
                &path,
                serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    let output = run(command, "git")?;
    if !output.status.success() {
        return Err(format_output("git", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn require_success(command: Command, label: &str) -> Result<(), String> {
    let output = run(command, label)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format_output(label, &output))
    }
}

fn run(mut command: Command, label: &str) -> Result<Output, String> {
    command.output().map_err(|e| format!("{label}: {e}"))
}

fn format_output(label: &str, output: &Output) -> String {
    format!(
        "{label} failed ({:?})\n{}{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("opui-{name}-{stamp}"))
    }

    fn repo() -> (PathBuf, String) {
        let root = temp("capsule-repo");
        fs::create_dir(&root).unwrap();
        git(&root, ["init"]).unwrap();
        fs::write(root.join("tracked"), "clean").unwrap();
        git(&root, ["add", "tracked"]).unwrap();
        let mut commit = Command::new("git");
        commit
            .args([
                "-c",
                "user.name=OPUI Test",
                "-c",
                "user.email=opui@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "fixture",
            ])
            .current_dir(&root);
        require_success(commit, "fixture commit").unwrap();
        let sha = git(&root, ["rev-parse", "HEAD"]).unwrap();
        (root, sha)
    }

    #[test]
    fn clean_clone_ignores_primary_dirt() {
        let (source, sha) = repo();
        fs::write(source.join("tracked"), "dirty").unwrap();
        let clone = temp("capsule-clone");
        clone_exact(&source, &clone, &sha).unwrap();
        validate_repo(&clone, &sha).unwrap();
        assert_eq!(fs::read_to_string(clone.join("tracked")).unwrap(), "clean");
        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(clone);
    }

    #[test]
    fn absent_or_wrong_submodule_fails_before_build() {
        let (root, sha) = repo();
        let missing = root.join("missing");
        assert!(
            validate_repo(&missing, &sha)
                .unwrap_err()
                .contains("missing")
        );
        assert!(
            validate_repo(&root, "deadbeef")
                .unwrap_err()
                .contains("expected")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_empty_submodule_placeholders_are_removed() {
        let empty = temp("empty-submodule");
        fs::create_dir(&empty).unwrap();
        remove_empty_submodule_placeholder(&empty).unwrap();
        assert!(!empty.exists());

        let nonempty = temp("stale-submodule");
        fs::create_dir(&nonempty).unwrap();
        fs::write(nonempty.join("stale"), "data").unwrap();
        assert!(
            remove_empty_submodule_placeholder(&nonempty)
                .unwrap_err()
                .contains("stale")
        );
        let _ = fs::remove_dir_all(nonempty);
    }

    #[test]
    fn stale_release_output_is_not_reused() {
        let source = temp("release-source").join("release-1");
        let stable = temp("release-stable");
        fs::create_dir_all(&source).unwrap();
        for file in ["complete", "release-report.json", "release-report.md"] {
            fs::write(source.join(file), "ok").unwrap();
        }
        promote_completed_bundle(&source, &stable).unwrap();
        assert!(
            promote_completed_bundle(&source, &stable)
                .unwrap_err()
                .contains("stale")
        );
        let _ = fs::remove_dir_all(source.parent().unwrap());
        let _ = fs::remove_dir_all(stable);
    }

    #[test]
    fn incomplete_release_is_never_promoted() {
        let source = temp("release-incomplete").join("release-1");
        let stable = temp("release-stable");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("release-report.json"), "{}").unwrap();
        assert!(
            promote_completed_bundle(&source, &stable)
                .unwrap_err()
                .contains("incomplete")
        );
        assert!(!stable.exists());
        let _ = fs::remove_dir_all(source.parent().unwrap());
    }

    #[test]
    fn failed_release_bundle_is_retained_without_completion() {
        let source = temp("release-failed").join("release-1");
        let stable = temp("release-stable");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("clean-aggregate.stderr"), "failed gate").unwrap();
        let retained = retain_failed_bundle(&source, &stable).unwrap();
        assert_eq!(
            fs::read_to_string(retained.join("clean-aggregate.stderr")).unwrap(),
            "failed gate"
        );
        assert!(!retained.join("complete").exists());
        let _ = fs::remove_dir_all(source.parent().unwrap());
        let _ = fs::remove_dir_all(stable);
    }

    #[test]
    fn promoted_json_paths_target_stable_evidence() {
        let root = temp("rewrite-paths");
        let from = root.join("temporary");
        let to = root.join("stable");
        fs::create_dir_all(&from).unwrap();
        fs::write(
            from.join("report.json"),
            serde_json::to_vec(&serde_json::json!({
                "path": from.join("case/report.json"),
                "other": "/unrelated/path"
            }))
            .unwrap(),
        )
        .unwrap();
        rewrite_json_paths(&from, &from, &to).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(from.join("report.json")).unwrap()).unwrap();
        assert_eq!(
            report["path"],
            to.join("case/report.json").to_string_lossy().as_ref()
        );
        assert_eq!(report["other"], "/unrelated/path");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn promoted_provenance_hashes_retained_manifest() {
        let root = temp("provenance-manifest");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("artifact-manifest.json"), "{\"artifacts\":[]}").unwrap();
        fs::write(
            root.join("provenance.json"),
            "{\"artifact_manifest_sha256\":\"stale\"}",
        )
        .unwrap();
        refresh_provenance_manifest_hash(&root).unwrap();
        let provenance: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("provenance.json")).unwrap()).unwrap();
        assert_eq!(
            provenance["artifact_manifest_sha256"],
            sha256_file(&root.join("artifact-manifest.json")).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }
}
