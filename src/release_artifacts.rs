use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::cert::sha256_file;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Artifact {
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactManifest {
    pub format_version: u32,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Serialize)]
pub struct ReproducibilityReport {
    pub independent_target_directories: bool,
    pub first: Vec<Artifact>,
    pub second: Vec<Artifact>,
    pub matching_hashes: bool,
    pub status: &'static str,
}

pub fn build_and_compare(root: &Path, release: &Path) -> Result<bool, String> {
    build_and_compare_with(
        root,
        release,
        crate::package_preflight::build_package_archives,
    )
}

pub fn build_and_compare_rc2(root: &Path, release: &Path) -> Result<bool, String> {
    build_and_compare_with(
        root,
        release,
        crate::package_preflight::build_rc2_package_archives,
    )
}

fn build_and_compare_with(
    root: &Path,
    release: &Path,
    build: fn(&Path, &Path) -> Result<Vec<std::path::PathBuf>, String>,
) -> Result<bool, String> {
    let first_dir = release.join("artifacts");
    let second_dir = release.join("reproducibility-second");
    let first = build_crates(root, &first_dir, build)?;
    let second = build_crates(root, &second_dir, build)?;
    let matching = first == second;
    let mut manifest_artifacts = first.clone();
    for artifact in &mut manifest_artifacts {
        artifact.file = format!("artifacts/{}", artifact.file);
    }
    let sbom = release.join("sbom.spdx.json");
    manifest_artifacts.push(artifact(&sbom, release)?);
    fs::write(
        release.join("artifact-manifest.json"),
        serde_json::to_vec_pretty(&ArtifactManifest {
            format_version: 1,
            artifacts: manifest_artifacts,
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        release.join("reproducibility.json"),
        serde_json::to_vec_pretty(&ReproducibilityReport {
            independent_target_directories: true,
            first,
            second,
            matching_hashes: matching,
            status: if matching { "pass" } else { "fail" },
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::remove_dir_all(second_dir).map_err(|e| e.to_string())?;
    Ok(matching)
}

pub fn refresh_sbom_artifact(release: &Path) -> Result<(), String> {
    let path = release.join("artifact-manifest.json");
    let mut manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let sbom = artifact(&release.join("sbom.spdx.json"), release)?;
    let entry = manifest
        .artifacts
        .iter_mut()
        .find(|entry| entry.file == "sbom.spdx.json")
        .ok_or("artifact manifest has no SBOM")?;
    *entry = sbom;
    fs::write(
        path,
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn write_unsigned_provenance(root: &Path, release: &Path) -> Result<(), String> {
    let lock: toml::Value = toml::from_str(
        &fs::read_to_string(root.join("repos.lock.toml")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let repositories = lock["repositories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|repo| Some((repo["id"].as_str()?, repo["sha"].as_str()?)))
        .map(|(id, sha)| (id.to_string(), sha.to_string()))
        .collect::<BTreeMap<_, _>>();
    let statement = serde_json::json!({
        "format_version": 1,
        "signed": false,
        "signature_status": "blocked-external-authority",
        "integration_commit": command(root, "git", &["rev-parse", "HEAD"])?,
        "integration_source_tree": lock["integration"]["source_tree"],
        "repositories": repositories,
        "toolchains": {
            "rustc": command(root, "rustc", &["--version"])?,
            "cargo": command(root, "cargo", &["--version"])?,
            "nix": command(root, "nix", &["--version"])?,
        },
        "artifact_manifest_sha256": sha256_file(&release.join("artifact-manifest.json"))?,
        "statement": "Locally generated unsigned provenance; cryptographic signing requires the external release authority.",
    });
    fs::write(
        release.join("provenance.json"),
        serde_json::to_vec_pretty(&statement).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn build_crates(
    root: &Path,
    destination: &Path,
    build: fn(&Path, &Path) -> Result<Vec<std::path::PathBuf>, String>,
) -> Result<Vec<Artifact>, String> {
    let crates = build(root, destination)?;
    if crates.len() != 2 {
        return Err(format!("expected 2 crate archives, found {}", crates.len()));
    }
    let mut artifacts = Vec::new();
    for package in crates {
        artifacts.push(artifact(&package, destination)?);
    }
    artifacts.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(artifacts)
}

fn artifact(path: &Path, relative_to: &Path) -> Result<Artifact, String> {
    Ok(Artifact {
        file: path
            .strip_prefix(relative_to)
            .map_err(|_| {
                format!(
                    "artifact {} is outside {}",
                    path.display(),
                    relative_to.display()
                )
            })?
            .to_string_lossy()
            .into_owned(),
        sha256: sha256_file(path)?,
        bytes: fs::metadata(path).map_err(|e| e.to_string())?.len(),
    })
}

fn command(root: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
