use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cert::sha256_file;
use crate::gate::{GateResult, GateStatus};
use crate::handoff::{CANDIDATE_SHA, MANIFEST_SHA256, RUNTIME_SHA256, SCHEMA_SHA256, VERSION};
use crate::release_profile::ReleaseProfile;

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
        self.results
            .iter()
            .map(|result| result.gate(root, source))
            .collect()
    }
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
    let temporary = destination.with_extension("toml.tmp");
    fs::write(
        &temporary,
        toml::to_string_pretty(&results).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
    Ok(destination)
}

fn safe_evidence_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "external evidence path must be repository-relative: {value}"
        ));
    }
    Ok(root.join(path))
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
}
