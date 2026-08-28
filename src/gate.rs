use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pass,
    Fail,
    Blocked,
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Release,
    ReleaseAsExperimental,
    DoNotRelease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CertificationVerdicts {
    pub case_conformance: GateStatus,
    pub environment_eligibility: GateStatus,
    pub case_verdict: Verdict,
    pub release_verdict: Verdict,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Release => "RELEASE",
            Self::ReleaseAsExperimental => "RELEASE AS EXPERIMENTAL",
            Self::DoNotRelease => "DO NOT RELEASE",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Release => 0,
            Self::ReleaseAsExperimental => 1,
            Self::DoNotRelease => 2,
        }
    }

    pub fn worse(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub id: String,
    pub status: GateStatus,
    pub required: bool,
    pub command: String,
    pub message: String,
    pub evidence: Vec<String>,
    pub input_hashes: BTreeMap<String, String>,
    pub output_hashes: BTreeMap<String, String>,
    pub duration_ms: u128,
    pub runner: BTreeMap<String, String>,
}

impl GateResult {
    pub fn new(id: impl Into<String>, required: bool) -> Self {
        Self {
            id: id.into(),
            status: GateStatus::Skipped,
            required,
            command: String::new(),
            message: String::new(),
            evidence: Vec::new(),
            input_hashes: BTreeMap::new(),
            output_hashes: BTreeMap::new(),
            duration_ms: 0,
            runner: BTreeMap::new(),
        }
    }

    pub fn finish(
        mut self,
        status: GateStatus,
        command: impl Into<String>,
        message: impl Into<String>,
        duration: Duration,
    ) -> Self {
        self.status = status;
        self.command = command.into();
        self.message = message.into();
        self.duration_ms = duration.as_millis();
        self
    }
}

pub fn fold_verdict(gates: &[GateResult], unauthorized_dirty: bool) -> Verdict {
    if unauthorized_dirty {
        return Verdict::DoNotRelease;
    }
    let mut v = Verdict::Release;
    for g in gates {
        if !g.required {
            continue;
        }
        v = match g.status {
            GateStatus::Fail => return Verdict::DoNotRelease,
            GateStatus::Blocked | GateStatus::Skipped => v.worse(Verdict::ReleaseAsExperimental),
            GateStatus::Pass => v,
        };
    }
    v
}

fn status_verdict(status: GateStatus) -> Verdict {
    match status {
        GateStatus::Pass => Verdict::Release,
        GateStatus::Fail => Verdict::DoNotRelease,
        GateStatus::Blocked | GateStatus::Skipped => Verdict::ReleaseAsExperimental,
    }
}

fn fold_status<'a>(gates: impl Iterator<Item = &'a GateResult>) -> GateStatus {
    let mut found = false;
    let mut status = GateStatus::Pass;
    for gate in gates.filter(|gate| gate.required) {
        found = true;
        status = match (status, gate.status) {
            (_, GateStatus::Fail) => return GateStatus::Fail,
            (GateStatus::Pass | GateStatus::Skipped, GateStatus::Blocked) => GateStatus::Blocked,
            (GateStatus::Pass, GateStatus::Skipped) => GateStatus::Skipped,
            (status, _) => status,
        };
    }
    if found { status } else { GateStatus::Skipped }
}

fn is_environment_gate(gate: &GateResult) -> bool {
    matches!(
        gate.id.as_str(),
        "repository_lock" | "clean_repository_state"
    )
}

pub fn classify_certification(
    gates: &[GateResult],
    unauthorized_dirty: bool,
    release_blocking: bool,
) -> CertificationVerdicts {
    let case_conformance = fold_status(gates.iter().filter(|gate| !is_environment_gate(gate)));
    let case_verdict = status_verdict(case_conformance);
    let environment = gates
        .iter()
        .filter(|gate| is_environment_gate(gate))
        .collect::<Vec<_>>();
    let environment_eligibility = if unauthorized_dirty {
        GateStatus::Fail
    } else if environment.len() != 2 {
        GateStatus::Blocked
    } else {
        match fold_status(environment.into_iter()) {
            GateStatus::Skipped => GateStatus::Blocked,
            status => status,
        }
    };
    let folded_case = if release_blocking || case_verdict == Verdict::Release {
        case_verdict
    } else {
        Verdict::ReleaseAsExperimental
    };
    let release_verdict = folded_case.worse(status_verdict(environment_eligibility));
    CertificationVerdicts {
        case_conformance,
        environment_eligibility,
        case_verdict,
        release_verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(id: &str, required: bool, status: GateStatus) -> GateResult {
        let mut r = GateResult::new(id, required);
        r.status = status;
        r
    }

    #[test]
    fn required_fail_is_do_not_release() {
        let gates = [g("visual", true, GateStatus::Fail)];
        assert_eq!(fold_verdict(&gates, false), Verdict::DoNotRelease);
    }

    #[test]
    fn required_blocked_is_experimental() {
        let gates = [g("reference", true, GateStatus::Blocked)];
        assert_eq!(fold_verdict(&gates, false), Verdict::ReleaseAsExperimental);
    }

    #[test]
    fn dirty_never_releases() {
        let gates = [g("ok", true, GateStatus::Pass)];
        assert_eq!(fold_verdict(&gates, true), Verdict::DoNotRelease);
    }

    #[test]
    fn missing_reference_is_not_pass() {
        let gates = [g("visual_compare", true, GateStatus::Skipped)];
        assert_ne!(fold_verdict(&gates, false), Verdict::Release);
    }

    fn classified(
        case_status: Option<GateStatus>,
        environment_status: GateStatus,
        dirty: bool,
        release_blocking: bool,
    ) -> CertificationVerdicts {
        let mut gates = vec![
            g("repository_lock", true, environment_status),
            g("clean_repository_state", true, environment_status),
        ];
        if let Some(status) = case_status {
            gates.push(g("visual_compare", true, status));
        }
        classify_certification(&gates, dirty, release_blocking)
    }

    #[test]
    fn allowed_dirt_does_not_fail_case_conformance() {
        let result = classified(Some(GateStatus::Pass), GateStatus::Blocked, false, true);
        assert_eq!(result.case_conformance, GateStatus::Pass);
        assert_eq!(result.environment_eligibility, GateStatus::Blocked);
        assert_eq!(result.release_verdict, Verdict::ReleaseAsExperimental);
    }

    #[test]
    fn dirty_environment_prevents_release() {
        let result = classified(Some(GateStatus::Pass), GateStatus::Fail, true, true);
        assert_eq!(result.case_verdict, Verdict::Release);
        assert_eq!(result.release_verdict, Verdict::DoNotRelease);
    }

    #[test]
    fn visual_failure_fails_case_conformance() {
        let result = classified(Some(GateStatus::Fail), GateStatus::Pass, false, true);
        assert_eq!(result.case_conformance, GateStatus::Fail);
        assert_eq!(result.case_verdict, Verdict::DoNotRelease);
    }

    #[test]
    fn non_release_blocking_failure_is_experimental() {
        let result = classified(Some(GateStatus::Fail), GateStatus::Pass, false, false);
        assert_eq!(result.case_verdict, Verdict::DoNotRelease);
        assert_eq!(result.release_verdict, Verdict::ReleaseAsExperimental);
    }

    #[test]
    fn release_blocking_failure_prevents_release() {
        let result = classified(Some(GateStatus::Fail), GateStatus::Pass, false, true);
        assert_eq!(result.release_verdict, Verdict::DoNotRelease);
    }

    #[test]
    fn missing_case_gate_cannot_pass() {
        let result = classified(None, GateStatus::Pass, false, true);
        assert_eq!(result.case_conformance, GateStatus::Skipped);
        assert_ne!(result.case_verdict, Verdict::Release);
    }
}
