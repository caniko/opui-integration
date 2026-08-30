use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cert::sha256_file;
use crate::gate::{GateResult, GateStatus};
use crate::handoff::{CANDIDATE_SHA, MANIFEST_SHA256, RUNTIME_SHA256, SCHEMA_SHA256, VERSION};
use crate::lock::load_lock;
use crate::public_source;
use crate::release_artifacts::ArtifactManifest;
use crate::release_profile::ReleaseProfile;

const RC2_VERSION: &str = "0.1.0-rc.2";
const RC2_SCHEMA_FILE: &str = "artifacts/openpencil_ui_schema-0.1.0-rc.2.crate";
const RC2_RUNTIME_FILE: &str = "artifacts/bevy_openpencil-0.1.0-rc.2.crate";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalResults {
    pub format_version: u32,
    pub candidate_version: String,
    pub candidate_source_sha: String,
    pub artifact_manifest_sha256: String,
    pub packages: Vec<ExternalPackage>,
    pub results: Vec<ExternalResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackage {
    pub name: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalResult {
    pub id: String,
    pub status: ExternalStatus,
    pub reason: String,
    pub required_environment: String,
    pub evidence: Vec<ExternalEvidence>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalStatus {
    Pass,
    Fail,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEvidence {
    pub path: String,
    pub sha256: String,
}

impl ExternalResults {
    pub fn load(path: &Path) -> Result<Self, String> {
        toml::from_str(&fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn validate(
        &self,
        root: &Path,
        profile: &ReleaseProfile,
        source: &Path,
    ) -> Result<Vec<GateResult>, String> {
        if self.format_version != 2 {
            return Err(format!(
                "unsupported external result format_version {}",
                self.format_version
            ));
        }
        if is_rc2(profile) {
            self.validate_rc2(root, profile, source)
        } else {
            self.validate_rc1(root, profile, source)
        }
    }

    fn validate_rc1(
        &self,
        root: &Path,
        profile: &ReleaseProfile,
        source: &Path,
    ) -> Result<Vec<GateResult>, String> {
        for (field, actual, expected) in [
            (
                "candidate_version",
                self.candidate_version.as_str(),
                VERSION,
            ),
            (
                "candidate_source_sha",
                self.candidate_source_sha.as_str(),
                CANDIDATE_SHA,
            ),
            (
                "artifact_manifest_sha256",
                self.artifact_manifest_sha256.as_str(),
                MANIFEST_SHA256,
            ),
        ] {
            if actual != expected {
                return Err(format!(
                    "stale or mismatched {field}: {actual} != {expected}"
                ));
            }
        }
        if profile.candidate_version != self.candidate_version {
            return Err("external result candidate does not match release profile".into());
        }
        exact_set(
            self.packages
                .iter()
                .map(|package| (package.name.as_str(), package.sha256.as_str())),
            [
                ("openpencil_ui_schema", SCHEMA_SHA256),
                ("bevy_openpencil", RUNTIME_SHA256),
            ],
            "package",
        )?;
        self.validate_gate_set(profile)?;
        self.results
            .iter()
            .map(|result| result.gate(root, source))
            .collect()
    }

    fn validate_rc2(
        &self,
        root: &Path,
        profile: &ReleaseProfile,
        source: &Path,
    ) -> Result<Vec<GateResult>, String> {
        let lock = load_lock(&root.join("repos.lock.toml"))?;
        let adapter = lock
            .repositories
            .iter()
            .find(|repo| repo.id == "bevy_openpencil")
            .ok_or("lock missing adapter repository")?;
        if self.candidate_version != profile.candidate_version
            || profile.candidate_version != RC2_VERSION
        {
            return Err("external result candidate does not match release profile".into());
        }
        if self.candidate_source_sha != adapter.sha {
            return Err(format!(
                "stale or mismatched candidate_source_sha: {} != {}",
                self.candidate_source_sha, adapter.sha
            ));
        }
        self.validate_gate_set(profile)?;
        let mut closure = None;
        for result in &self.results {
            if result.id == "public_source_closure" {
                if closure.is_some() || !matches!(result.status, ExternalStatus::Pass) {
                    return Err("RC2 requires exactly one public_source_closure Pass".into());
                }
                closure = Some(result);
            } else if !matches!(result.status, ExternalStatus::Blocked) {
                return Err(format!("external gate {} must remain Blocked", result.id));
            }
        }
        let closure = closure.ok_or("RC2 requires exactly one public_source_closure Pass")?;
        if closure.evidence.len() != 2 {
            return Err(
                "public_source_closure requires transcript and artifact-manifest evidence".into(),
            );
        }
        let mut transcript = None;
        let mut manifest_ev = None;
        for evidence in &closure.evidence {
            let path = safe_evidence_path(root, &evidence.path)?;
            if evidence.path.rsplit('/').next() == Some("artifact-manifest.json") {
                if manifest_ev.is_some() {
                    return Err("public_source_closure has extra artifact-manifest evidence".into());
                }
                manifest_ev = Some((path, evidence));
            } else if transcript.is_some() {
                return Err("public_source_closure has extra transcript evidence".into());
            } else {
                transcript = Some((path, evidence));
            }
        }
        let (manifest_path, manifest_ev) =
            manifest_ev.ok_or("public_source_closure missing artifact-manifest.json evidence")?;
        let (transcript_path, _) =
            transcript.ok_or("public_source_closure missing transcript evidence")?;
        let manifest_hash = sha256_file(&manifest_path)?;
        if manifest_hash != manifest_ev.sha256 || manifest_hash != self.artifact_manifest_sha256 {
            return Err(format!(
                "stale or mismatched artifact_manifest_sha256: {} != {manifest_hash}",
                self.artifact_manifest_sha256
            ));
        }
        let manifest: ArtifactManifest = serde_json::from_slice(
            &fs::read(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
        crate::lock::full_sha256(&self.artifact_manifest_sha256)?;
        let expected_packages = packages_from_manifest(&manifest)?;
        if self.packages.len() != expected_packages.len() {
            return Err("external package set mismatch".into());
        }
        for (package, (name, sha256)) in self.packages.iter().zip(&expected_packages) {
            crate::lock::full_sha256(&package.sha256)?;
            if package.name != *name || package.sha256 != *sha256 {
                return Err("external package set mismatch".into());
            }
        }
        let gates = self
            .results
            .iter()
            .map(|result| result.gate(root, source))
            .collect::<Result<Vec<_>, _>>()?;
        public_source::validate_live(root, &transcript_path)?;
        Ok(gates)
    }

    fn validate_gate_set(&self, profile: &ReleaseProfile) -> Result<(), String> {
        let expected_ids: BTreeSet<_> = profile.external_gates.iter().map(String::as_str).collect();
        let actual_ids: BTreeSet<_> = self
            .results
            .iter()
            .map(|result| result.id.as_str())
            .collect();
        if actual_ids.len() != self.results.len() || actual_ids != expected_ids {
            return Err(format!(
                "external gate set mismatch: expected {expected_ids:?}, got {actual_ids:?}"
            ));
        }
        Ok(())
    }
}

pub fn generate_rc2(
    root: &Path,
    capsule: &Path,
    closure: &Path,
    output: &Path,
) -> Result<PathBuf, String> {
    let profile = ReleaseProfile::load(root, "rc2")?;
    let lock = load_lock(&root.join("repos.lock.toml"))?;
    let adapter = lock
        .repositories
        .iter()
        .find(|repo| repo.id == "bevy_openpencil")
        .ok_or("lock missing adapter repository")?;
    let capsule = if capsule.is_absolute() {
        capsule.to_path_buf()
    } else {
        root.join(capsule)
    };
    let closure = if closure.is_absolute() {
        closure.to_path_buf()
    } else {
        root.join(closure)
    };
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    if output.exists() {
        return Err(format!(
            "external result output already exists: {}",
            output.display()
        ));
    }
    let temporary = output.with_extension("toml.tmp");
    if temporary.exists() {
        return Err(format!(
            "stale external result stage {}",
            temporary.display()
        ));
    }
    let manifest_path = capsule.join("artifact-manifest.json");
    let artifact_manifest_sha256 = sha256_file(&manifest_path)?;
    let manifest: ArtifactManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|e| format!("{}: {e}", manifest_path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let packages = packages_from_manifest(&manifest)?
        .into_iter()
        .map(|(name, sha256)| ExternalPackage { name, sha256 })
        .collect();
    let mut results = vec![ExternalResult {
        id: "public_source_closure".into(),
        status: ExternalStatus::Pass,
        reason: "public source closure observed on GitHub-hosted workflow_dispatch".into(),
        required_environment: "GitHub-hosted workflow_dispatch".into(),
        evidence: vec![
            ExternalEvidence {
                path: rel(root, &closure)?,
                sha256: sha256_file(&closure)?,
            },
            ExternalEvidence {
                path: rel(root, &manifest_path)?,
                sha256: artifact_manifest_sha256.clone(),
            },
        ],
    }];
    for (id, reason, environment) in [
        (
            "windows_native",
            "Windows native runner required",
            "windows native runner",
        ),
        (
            "macos_native",
            "macOS native runner required",
            "macos native runner",
        ),
        (
            "physical_gamepad",
            "supported physical gamepad required",
            "physical gamepad",
        ),
        (
            "physical_touch",
            "physical touch display required",
            "physical touch display",
        ),
        (
            "provenance_signature",
            "authorized release signing identity required",
            "authorized signing identity",
        ),
    ] {
        results.push(ExternalResult {
            id: id.into(),
            status: ExternalStatus::Blocked,
            reason: reason.into(),
            required_environment: environment.into(),
            evidence: vec![],
        });
    }
    let results = ExternalResults {
        format_version: 2,
        candidate_version: profile.candidate_version.clone(),
        candidate_source_sha: adapter.sha.clone(),
        artifact_manifest_sha256,
        packages,
        results,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &temporary,
        toml::to_string_pretty(&results).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let written = match ExternalResults::load(&temporary) {
        Ok(written) => written,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    if let Err(error) = written.validate(root, &profile, &temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, &output).map_err(|e| e.to_string()) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(output)
}

impl ExternalResult {
    fn gate(&self, root: &Path, source: &Path) -> Result<GateResult, String> {
        if self.reason.trim().is_empty() || self.required_environment.trim().is_empty() {
            return Err(format!(
                "external gate {} requires a reason and environment",
                self.id
            ));
        }
        if matches!(self.status, ExternalStatus::Pass) && self.evidence.is_empty() {
            return Err(format!("passing external gate {} has no evidence", self.id));
        }
        let mut gate = GateResult::new(&self.id, true);
        gate.evidence.push(source.display().to_string());
        for evidence in &self.evidence {
            let path = safe_evidence_path(root, &evidence.path)?;
            let actual = sha256_file(&path)?;
            if actual != evidence.sha256 {
                return Err(format!(
                    "external evidence checksum mismatch for {}: {actual} != {}",
                    evidence.path, evidence.sha256
                ));
            }
            gate.evidence.push(path.display().to_string());
            gate.output_hashes.insert(evidence.path.clone(), actual);
        }
        Ok(gate.finish(
            match self.status {
                ExternalStatus::Pass => GateStatus::Pass,
                ExternalStatus::Fail => GateStatus::Fail,
                ExternalStatus::Blocked => GateStatus::Blocked,
            },
            format!("external execution on {}", self.required_environment),
            &self.reason,
            Duration::ZERO,
        ))
    }
}

pub fn load_gates(root: &Path, profile: &ReleaseProfile) -> Result<Vec<GateResult>, String> {
    let path = root.join(format!("external-results/{}.toml", profile.id));
    ExternalResults::load(&path)?.validate(root, profile, &path)
}

pub fn import(root: &Path, profile_id: &str, source: &Path) -> Result<PathBuf, String> {
    let profile = ReleaseProfile::load(root, profile_id)?;
    let results = ExternalResults::load(source)?;
    results.validate(root, &profile, source)?;
    let destination = root.join(format!("external-results/{profile_id}.toml"));
    fs::create_dir_all(root.join("external-results")).map_err(|e| e.to_string())?;
    let temporary = destination.with_extension("toml.tmp");
    fs::write(
        &temporary,
        toml::to_string_pretty(&results).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

fn is_rc2(profile: &ReleaseProfile) -> bool {
    profile.id == "rc2" || profile.candidate_version == RC2_VERSION
}

fn packages_from_manifest(manifest: &ArtifactManifest) -> Result<Vec<(String, String)>, String> {
    let mut schema = None;
    let mut runtime = None;
    for artifact in &manifest.artifacts {
        if artifact.file == RC2_SCHEMA_FILE {
            if schema.is_some() {
                return Err("duplicate schema package artifact".into());
            }
            crate::lock::full_sha256(&artifact.sha256)?;
            schema = Some(("openpencil_ui_schema".to_string(), artifact.sha256.clone()));
        } else if artifact.file == RC2_RUNTIME_FILE {
            if runtime.is_some() {
                return Err("duplicate runtime package artifact".into());
            }
            crate::lock::full_sha256(&artifact.sha256)?;
            runtime = Some(("bevy_openpencil".to_string(), artifact.sha256.clone()));
        }
    }
    match (schema, runtime) {
        (Some(schema), Some(runtime)) => Ok(vec![schema, runtime]),
        _ => Err("external package set mismatch".into()),
    }
}

fn rel(root: &Path, path: &Path) -> Result<String, String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| {
            format!(
                "external evidence path must be repository-relative: {}",
                path.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");
    safe_evidence_path(root, &rel)?;
    Ok(rel)
}

fn safe_evidence_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    public_source::confined_repo_file(root, value)
}

fn exact_set<'a>(
    actual: impl Iterator<Item = (&'a str, &'a str)>,
    expected: impl IntoIterator<Item = (&'a str, &'a str)>,
    label: &str,
) -> Result<(), String> {
    let actual: Vec<_> = actual.collect();
    let actual_len = actual.len();
    let actual: BTreeMap<_, _> = actual.into_iter().collect();
    let expected: BTreeMap<_, _> = expected.into_iter().collect();
    if actual_len != expected.len() || actual != expected {
        return Err(format!("external {label} set mismatch"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> ReleaseProfile {
        ReleaseProfile {
            id: "rc1".into(),
            candidate_version: VERSION.into(),
            required_gates: vec!["owned".into()],
            external_gates: vec!["external".into()],
            allow_blocked_external: true,
        }
    }

    fn results(status: ExternalStatus) -> ExternalResults {
        ExternalResults {
            format_version: 2,
            candidate_version: VERSION.into(),
            candidate_source_sha: CANDIDATE_SHA.into(),
            artifact_manifest_sha256: MANIFEST_SHA256.into(),
            packages: vec![
                ExternalPackage {
                    name: "openpencil_ui_schema".into(),
                    sha256: SCHEMA_SHA256.into(),
                },
                ExternalPackage {
                    name: "bevy_openpencil".into(),
                    sha256: RUNTIME_SHA256.into(),
                },
            ],
            results: vec![ExternalResult {
                id: "external".into(),
                status,
                reason: "not available".into(),
                required_environment: "external runner".into(),
                evidence: vec![],
            }],
        }
    }

    #[test]
    fn blocked_is_distinct_from_failed() {
        let blocked = results(ExternalStatus::Blocked)
            .validate(Path::new("."), &profile(), Path::new("results.toml"))
            .unwrap();
        let failed = results(ExternalStatus::Fail)
            .validate(Path::new("."), &profile(), Path::new("results.toml"))
            .unwrap();
        assert_eq!(blocked[0].status, GateStatus::Blocked);
        assert_eq!(failed[0].status, GateStatus::Fail);
    }

    #[test]
    fn stale_missing_and_unhashed_results_are_rejected() {
        let mut stale = results(ExternalStatus::Blocked);
        stale.candidate_source_sha = "old".into();
        assert!(
            stale
                .validate(Path::new("."), &profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("mismatched")
        );

        let missing = ExternalResults {
            results: vec![],
            ..results(ExternalStatus::Blocked)
        };
        assert!(
            missing
                .validate(Path::new("."), &profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("gate set")
        );
        assert!(
            results(ExternalStatus::Pass)
                .validate(Path::new("."), &profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("no evidence")
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = r#"format_version = 2
candidate_version = "0.1.0-rc.1"
candidate_source_sha = "sha"
artifact_manifest_sha256 = "hash"
unexpected = true
packages = []
results = []
"#;
        assert!(toml::from_str::<ExternalResults>(text).is_err());
    }

    #[test]
    fn passing_evidence_must_match_its_hash() {
        let root =
            std::env::temp_dir().join(format!("opui-external-results-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("evidence.log"), b"pass\n").unwrap();
        let mut result = results(ExternalStatus::Pass);
        result.results[0].evidence.push(ExternalEvidence {
            path: "evidence.log".into(),
            sha256: sha256_file(&root.join("evidence.log")).unwrap(),
        });
        assert_eq!(
            result
                .validate(&root, &profile(), Path::new("results.toml"))
                .unwrap()[0]
                .status,
            GateStatus::Pass
        );
        result.results[0].evidence[0].sha256 = "wrong".into();
        assert!(
            result
                .validate(&root, &profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("checksum mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_creates_external_results_directory() {
        let root =
            std::env::temp_dir().join(format!("opui-external-import-{}", std::process::id()));
        fs::create_dir_all(root.join("release-profiles")).unwrap();
        fs::write(
            root.join("release-profiles/rc1.toml"),
            toml::to_string(&profile()).unwrap(),
        )
        .unwrap();
        let source = root.join("result.toml");
        fs::write(
            &source,
            toml::to_string(&results(ExternalStatus::Blocked)).unwrap(),
        )
        .unwrap();

        assert!(import(&root, "rc1", &source).unwrap().is_file());

        fs::remove_dir_all(root).unwrap();
    }

    fn rc2_profile() -> ReleaseProfile {
        ReleaseProfile {
            id: "rc2".into(),
            candidate_version: RC2_VERSION.into(),
            required_gates: vec!["owned".into()],
            external_gates: vec![
                "public_source_closure".into(),
                "windows_native".into(),
                "macos_native".into(),
                "physical_gamepad".into(),
                "physical_touch".into(),
                "provenance_signature".into(),
            ],
            allow_blocked_external: true,
        }
    }

    fn adapter_sha() -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        load_lock(&root.join("repos.lock.toml"))
            .unwrap()
            .repositories
            .into_iter()
            .find(|repo| repo.id == "bevy_openpencil")
            .unwrap()
            .sha
    }

    fn blocked(id: &str) -> ExternalResult {
        ExternalResult {
            id: id.into(),
            status: ExternalStatus::Blocked,
            reason: "not available".into(),
            required_environment: "external runner".into(),
            evidence: vec![],
        }
    }

    fn rc2_results() -> ExternalResults {
        ExternalResults {
            format_version: 2,
            candidate_version: RC2_VERSION.into(),
            candidate_source_sha: adapter_sha(),
            artifact_manifest_sha256: "ab".repeat(32),
            packages: vec![],
            results: vec![
                ExternalResult {
                    id: "public_source_closure".into(),
                    status: ExternalStatus::Pass,
                    reason: "observed".into(),
                    required_environment: "GitHub-hosted workflow_dispatch".into(),
                    evidence: vec![
                        ExternalEvidence {
                            path: "handoff/closure.json".into(),
                            sha256: "cd".repeat(32),
                        },
                        ExternalEvidence {
                            path: "artifact-manifest.json".into(),
                            sha256: "ab".repeat(32),
                        },
                    ],
                },
                blocked("windows_native"),
                blocked("macos_native"),
                blocked("physical_gamepad"),
                blocked("physical_touch"),
                blocked("provenance_signature"),
            ],
        }
    }

    #[test]
    fn rc1_identity_still_uses_handoff_constants() {
        let mut stale = results(ExternalStatus::Blocked);
        stale.candidate_version = RC2_VERSION.into();
        assert!(
            stale
                .validate(Path::new("."), &profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("mismatched")
        );
    }

    #[test]
    fn rc2_identity_rejects_stale_adapter_and_missing_closure() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut stale = rc2_results();
        stale.candidate_source_sha = CANDIDATE_SHA.into();
        assert!(
            stale
                .validate(root, &rc2_profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("mismatched")
        );

        let mut version = rc2_results();
        version.candidate_version = VERSION.into();
        assert!(
            version
                .validate(root, &rc2_profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("does not match")
        );

        let mut missing = rc2_results();
        missing.results[0].status = ExternalStatus::Blocked;
        missing.results[0].evidence.clear();
        assert!(
            missing
                .validate(root, &rc2_profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("public_source_closure")
        );

        let mut unsafe_path = rc2_results();
        unsafe_path.results[0].evidence[0].path = "../secret".into();
        assert!(
            unsafe_path
                .validate(root, &rc2_profile(), Path::new("results.toml"))
                .unwrap_err()
                .contains("repository-relative")
        );
    }

    fn artifact(file: &str, sha: &str) -> crate::release_artifacts::Artifact {
        crate::release_artifacts::Artifact {
            file: file.into(),
            sha256: sha.into(),
            bytes: 1,
        }
    }

    #[test]
    fn packages_from_manifest_require_exact_unique_rc2_files() {
        let ok = ArtifactManifest {
            format_version: 1,
            artifacts: vec![
                artifact(RC2_SCHEMA_FILE, &"ab".repeat(32)),
                artifact(RC2_RUNTIME_FILE, &"cd".repeat(32)),
            ],
        };
        let packages = packages_from_manifest(&ok).unwrap();
        assert_eq!(packages[0].0, "openpencil_ui_schema");
        assert_eq!(packages[1].0, "bevy_openpencil");

        let prefix = ArtifactManifest {
            format_version: 1,
            artifacts: vec![
                artifact("openpencil_ui_schema-0.1.0-rc.2.crate", &"ab".repeat(32)),
                artifact(RC2_RUNTIME_FILE, &"cd".repeat(32)),
            ],
        };
        assert!(packages_from_manifest(&prefix).is_err());

        let dup = ArtifactManifest {
            format_version: 1,
            artifacts: vec![
                artifact(RC2_SCHEMA_FILE, &"ab".repeat(32)),
                artifact(RC2_SCHEMA_FILE, &"ef".repeat(32)),
                artifact(RC2_RUNTIME_FILE, &"cd".repeat(32)),
            ],
        };
        assert!(
            packages_from_manifest(&dup)
                .unwrap_err()
                .contains("duplicate")
        );

        let upper = ArtifactManifest {
            format_version: 1,
            artifacts: vec![
                artifact(RC2_SCHEMA_FILE, &"AB".repeat(32)),
                artifact(RC2_RUNTIME_FILE, &"cd".repeat(32)),
            ],
        };
        assert!(packages_from_manifest(&upper).is_err());
    }

    #[test]
    fn generate_rc2_rejects_stale_stage() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = std::env::temp_dir().join(format!("opui-rc2-gen-{}.toml", std::process::id()));
        let temporary = output.with_extension("toml.tmp");
        let _ = fs::remove_file(&output);
        let _ = fs::remove_file(&temporary);
        fs::write(&temporary, b"stale").unwrap();
        let err = generate_rc2(
            root,
            Path::new("missing-capsule"),
            Path::new("missing-closure"),
            &output,
        )
        .unwrap_err();
        assert!(err.contains("stale external result stage"), "{err}");
        fs::remove_file(temporary).unwrap();
    }
}
