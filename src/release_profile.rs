use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::gate::{GateResult, GateStatus, Verdict};

#[derive(Debug, Deserialize, Serialize)]
pub struct ReleaseProfile {
    pub id: String,
    pub candidate_version: String,
    pub required_gates: Vec<String>,
    pub external_gates: Vec<String>,
    #[serde(default = "allow_blocked_external")]
    pub allow_blocked_external: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileEvaluation {
    pub profile: String,
    pub candidate_version: String,
    pub verdict: Verdict,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReadinessAssessment {
    pub schema_version: u32,
    pub readiness: &'static str,
    pub source_report_sha256: String,
    pub profile_evaluation: ProfileEvaluation,
    pub external_status: BTreeMap<String, GateStatus>,
}

impl ReleaseProfile {
    pub fn load(root: &Path, id: &str) -> Result<Self, String> {
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(format!("invalid release profile {id:?}"));
        }
        let path = root.join(format!("release-profiles/{id}.toml"));
        let profile: Self = toml::from_str(
            &fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", path.display()))?;
        if profile.id != id {
            return Err(format!(
                "profile {} declares id {}",
                path.display(),
                profile.id
            ));
        }
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        for id in self.required_gates.iter().chain(&self.external_gates) {
            if !ids.insert(id) {
                return Err(format!("profile {} declares duplicate gate {id}", self.id));
            }
        }
        if self.required_gates.is_empty() {
            return Err(format!("profile {} has no required gates", self.id));
        }
        Ok(())
    }

    pub fn evaluate(&self, gates: &[GateResult]) -> ProfileEvaluation {
        let actual: BTreeMap<&str, GateStatus> = gates
            .iter()
            .map(|gate| (gate.id.as_str(), gate.status))
            .collect();
        let mut issues = Vec::new();
        for id in &self.required_gates {
            match actual.get(id.as_str()) {
                Some(GateStatus::Pass) => {}
                Some(status) => issues.push(format!("required gate {id} is {status:?}")),
                None => issues.push(format!("required gate {id} is missing")),
            }
        }
        for id in &self.external_gates {
            match actual.get(id.as_str()) {
                Some(GateStatus::Pass) => {}
                Some(GateStatus::Blocked) if self.allow_blocked_external => {}
                Some(status) => issues.push(format!("external gate {id} is {status:?}")),
                None => issues.push(format!("external gate {id} is missing")),
            }
        }
        ProfileEvaluation {
            profile: self.id.clone(),
            candidate_version: self.candidate_version.clone(),
            verdict: if issues.is_empty() {
                Verdict::Release
            } else {
                Verdict::DoNotRelease
            },
            issues,
        }
    }
}

const fn allow_blocked_external() -> bool {
    true
}

pub fn assess_capsule(
    root: &Path,
    profile_id: &str,
    capsule: &Path,
    output: &Path,
) -> Result<PathBuf, String> {
    if output.exists() {
        return Err(format!(
            "assessment output already exists: {}",
            output.display()
        ));
    }
    let profile = ReleaseProfile::load(root, profile_id)?;
    let report_path = capsule.join("release-report.json");
    let report: serde_json::Value = serde_json::from_slice(
        &fs::read(&report_path).map_err(|e| format!("{}: {e}", report_path.display()))?,
    )
    .map_err(|e| e.to_string())?;
    if report["verdict"] != "RELEASE"
        || report["profile"]["candidate_version"] != profile.candidate_version
    {
        return Err("certified capsule is failed or belongs to another candidate".into());
    }
    let mut gates: Vec<GateResult> =
        serde_json::from_value(report["release_gates"].clone()).map_err(|e| e.to_string())?;
    gates.retain(|gate| !profile.external_gates.contains(&gate.id));
    gates.extend(crate::external_results::load_gates(root, &profile)?);
    let evaluation = profile.evaluate(&gates);
    let external_status: BTreeMap<_, _> = gates
        .iter()
        .filter(|gate| profile.external_gates.contains(&gate.id))
        .map(|gate| (gate.id.clone(), gate.status))
        .collect();
    let owned_pass = profile.required_gates.iter().all(|id| {
        gates
            .iter()
            .any(|gate| gate.id == *id && gate.status == GateStatus::Pass)
    });
    let readiness = classify_readiness(owned_pass, external_status.values().copied());
    let assessment = ReadinessAssessment {
        schema_version: 1,
        readiness,
        source_report_sha256: crate::cert::sha256_file(&report_path)?,
        profile_evaluation: evaluation,
        external_status,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        output,
        serde_json::to_vec_pretty(&assessment).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(output.to_path_buf())
}

fn classify_readiness(
    owned_pass: bool,
    external: impl IntoIterator<Item = GateStatus>,
) -> &'static str {
    let external: Vec<_> = external.into_iter().collect();
    if !owned_pass
        || external
            .iter()
            .any(|status| matches!(status, GateStatus::Fail | GateStatus::Skipped))
    {
        "fail"
    } else if external.contains(&GateStatus::Blocked) {
        "blocked"
    } else {
        "ready"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(id: &str, status: GateStatus) -> GateResult {
        let mut gate = GateResult::new(id, true);
        gate.status = status;
        gate
    }

    fn profile() -> ReleaseProfile {
        ReleaseProfile {
            id: "test".into(),
            candidate_version: "1.0.0".into(),
            required_gates: vec!["owned".into()],
            external_gates: vec!["external".into()],
            allow_blocked_external: true,
        }
    }

    #[test]
    fn owned_gate_must_pass() {
        let evaluation = profile().evaluate(&[
            gate("owned", GateStatus::Blocked),
            gate("external", GateStatus::Blocked),
        ]);
        assert_eq!(evaluation.verdict, Verdict::DoNotRelease);
    }

    #[test]
    fn unavailable_external_gate_may_be_blocked() {
        let evaluation = profile().evaluate(&[
            gate("owned", GateStatus::Pass),
            gate("external", GateStatus::Blocked),
        ]);
        assert_eq!(evaluation.verdict, Verdict::Release);
    }

    #[test]
    fn missing_or_skipped_gate_never_releases() {
        assert_eq!(
            profile()
                .evaluate(&[gate("owned", GateStatus::Pass)])
                .verdict,
            Verdict::DoNotRelease
        );
        assert_eq!(
            profile()
                .evaluate(&[
                    gate("owned", GateStatus::Pass),
                    gate("external", GateStatus::Skipped),
                ])
                .verdict,
            Verdict::DoNotRelease
        );
    }

    #[test]
    fn profile_rejects_duplicate_classification() {
        let mut profile = profile();
        profile.external_gates.push("owned".into());
        assert!(profile.validate().is_err());
    }

    #[test]
    fn stable_profile_rejects_blocked_external_gate() {
        let mut profile = profile();
        profile.allow_blocked_external = false;
        assert_eq!(
            profile
                .evaluate(&[
                    gate("owned", GateStatus::Pass),
                    gate("external", GateStatus::Blocked),
                ])
                .verdict,
            Verdict::DoNotRelease
        );
    }

    #[test]
    fn stable_profile_is_strict_on_blocked_external_gates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let profile = ReleaseProfile::load(root, "stable-v1").unwrap();
        assert!(!profile.allow_blocked_external);
        assert_eq!(profile.candidate_version, "0.1.0-rc.1");
    }

    #[test]
    fn rc2_profile_keeps_blocked_external_gates_explicit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let profile = ReleaseProfile::load(root, "rc2").unwrap();
        assert!(profile.allow_blocked_external);
        assert_eq!(profile.candidate_version, "0.1.0-rc.2");
    }

    #[test]
    fn readiness_distinguishes_blocked_from_failed() {
        assert_eq!(classify_readiness(true, [GateStatus::Pass]), "ready");
        assert_eq!(classify_readiness(true, [GateStatus::Blocked]), "blocked");
        assert_eq!(classify_readiness(true, [GateStatus::Fail]), "fail");
        assert_eq!(classify_readiness(false, [GateStatus::Pass]), "fail");
    }
}
