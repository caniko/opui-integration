use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::cert::sha256_file;

pub const VERSION: &str = "0.1.0-rc.1";
pub const CANDIDATE_SHA: &str = "1a00b72b6bd333e67c1c5224a07d8d81881ca30d";
pub const MANIFEST_SHA256: &str =
    "552bd00dd3fa96ed2ccbac2851bfeb1a3a6d21884cf408866323c407d425f0ca";
pub const SCHEMA_SHA256: &str = "ec76e68ce27bdc136916da9dcca79900fe8e664277c7ba3fa18df87b08edf69f";
pub const RUNTIME_SHA256: &str = "c898fc4c0cf822b0bb77e812dc75af87105ede1dc1cf54a3888e1758326e5373";
pub const VERITASIUM_SHA: &str = "7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0";
const OPUI_SHA: &str = "04fdda1c8a2dabd4fad3ee66dd9043f44ed8509c";
const OPENPENCIL_SHA: &str = "4c2a37e3d6632c89530f0edcfd7aec184e38383f";
const AGENT_SHA: &str = "5c0f9506e27be5b4f29cd2c1093e2858ad0fa20c";
const CASEMENT_SHA: &str = "451173eb3353a4166d2d0f241f2e0606051064bd";
const JIAN_SHA: &str = "ba334d27edf05b7e4c7a2746fc3c664d9ed24f28";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffManifest {
    pub schema_version: u32,
    pub candidate_version: String,
    pub candidate_source_sha: String,
    pub authoritative_capsule: String,
    pub artifact_manifest_sha256: String,
    pub packages: Vec<HandoffPackage>,
    pub repositories: BTreeMap<String, String>,
    pub cargo_locks: BTreeMap<String, String>,
    pub evidence: BTreeMap<String, String>,
    pub evidence_summary: EvidenceSummary,
    pub external_status: BTreeMap<String, String>,
    pub publication_status: String,
    pub signing_status: String,
    pub hosted_ci_status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSummary {
    pub visual_cases_passed: usize,
    pub visual_cases_total: usize,
    pub accessibility_snapshots: usize,
    pub accessibility_nodes: usize,
    pub graphical_stress_status: String,
    pub graphical_stress_cycles: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffPackage {
    pub name: String,
    pub version: String,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
struct SigningPayload {
    schema_version: u32,
    candidate_version: String,
    candidate_source_sha: String,
    status: String,
    authority: String,
    subjects: Vec<SigningSubject>,
}

#[derive(Debug, Serialize)]
struct SigningSubject {
    name: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct FinalHandoff {
    schema_version: u32,
    status: &'static str,
    candidate_version: &'static str,
    candidate_source_sha: &'static str,
    generator_commit: String,
    publication_performed: bool,
    handoff_manifest_sha256: String,
    signing_payload_sha256: String,
    publication_rehearsal_sha256: String,
    stable_v1_assessment_sha256: String,
}

#[derive(Deserialize)]
struct ArtifactManifest {
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    file: String,
    sha256: String,
    bytes: u64,
}

pub fn prepare(root: &Path, capsule: &Path, destination: &Path) -> Result<PathBuf, String> {
    validate_source(root)?;
    let integration_sha = command(root, "git", &["rev-parse", "HEAD"])?;
    let (artifact_manifest, provenance, report, capsule_metadata) =
        validate_capsule(capsule, &integration_sha)?;
    if destination.exists() {
        return Err(format!(
            "handoff destination already exists: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;

    let files = [
        "artifact-manifest.json",
        "capsule.json",
        "paired-clean.json",
        "performance.json",
        "provenance.json",
        "public_api_runtime.log",
        "public_api_schema.log",
        "release-report.json",
        "release-report.md",
        "reproducibility.json",
        "sbom.spdx.json",
        "semver_runtime.log",
        "semver_schema.log",
    ];
    for file in files {
        copy(capsule.join(file), destination.join(file))?;
    }
    fs::create_dir(destination.join("artifacts")).map_err(|e| e.to_string())?;
    for package in [
        "openpencil_ui_schema-0.1.0-rc.1.crate",
        "bevy_openpencil-0.1.0-rc.1.crate",
    ] {
        copy(
            capsule.join("artifacts").join(package),
            destination.join("artifacts").join(package),
        )?;
    }
    copy(
        capsule.join("graphical-stress/stress.json"),
        destination.join("graphical-stress.json"),
    )?;
    copy(
        root.join("external-results/rc1.toml"),
        destination.join("external-results.toml"),
    )?;
    copy(
        root.join("docs/rc1-publication-runbook.md"),
        destination.join("publication-runbook.md"),
    )?;
    copy(
        root.join("docs/rc1-candidate-audit.md"),
        destination.join("candidate-audit.md"),
    )?;

    let mut evidence = BTreeMap::new();
    for file in files
        .into_iter()
        .chain(["graphical-stress.json", "external-results.toml"])
    {
        evidence.insert(file.into(), sha256_file(&destination.join(file))?);
    }
    let packages = packages(&artifact_manifest)?;
    let repositories = source_manifest(&provenance, &capsule_metadata)?;
    let cargo_locks = cargo_locks(root)?;
    let evidence_summary = evidence_summary(capsule, &report)?;
    let external_status = report["release_gates"]
        .as_array()
        .ok_or("release report has no gates")?
        .iter()
        .filter_map(|gate| {
            let id = gate["id"].as_str()?;
            matches!(
                id,
                "windows_native"
                    | "macos_native"
                    | "physical_gamepad"
                    | "physical_touch"
                    | "provenance_signature"
            )
            .then(|| {
                (
                    id.to_string(),
                    gate["status"].as_str().unwrap_or("fail").into(),
                )
            })
        })
        .collect();
    let manifest = HandoffManifest {
        schema_version: 2,
        candidate_version: VERSION.into(),
        candidate_source_sha: CANDIDATE_SHA.into(),
        authoritative_capsule: capsule.display().to_string(),
        artifact_manifest_sha256: MANIFEST_SHA256.into(),
        packages,
        repositories,
        cargo_locks,
        evidence,
        evidence_summary,
        external_status,
        publication_status: "not-published".into(),
        signing_status: "unsigned-external-authority-blocked".into(),
        hosted_ci_status: "blocked-no-configured-provider".into(),
    };
    let output = destination.join("rc1-handoff.json");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("rc1-handoff.sha256"),
        format!("{}  rc1-handoff.json\n", sha256_file(&output)?),
    )
    .map_err(|e| e.to_string())?;
    let signing_payload = SigningPayload {
        schema_version: 1,
        candidate_version: VERSION.into(),
        candidate_source_sha: CANDIDATE_SHA.into(),
        status: "unsigned".into(),
        authority: "external-release-authority".into(),
        subjects: [
            "rc1-handoff.json",
            "artifact-manifest.json",
            "artifacts/openpencil_ui_schema-0.1.0-rc.1.crate",
            "artifacts/bevy_openpencil-0.1.0-rc.1.crate",
            "sbom.spdx.json",
        ]
        .into_iter()
        .map(|name| {
            Ok(SigningSubject {
                name: name.into(),
                sha256: sha256_file(&destination.join(name))?,
            })
        })
        .collect::<Result<_, String>>()?,
    };
    let signing_path = destination.join("signing-payload.json");
    fs::write(
        &signing_path,
        serde_json::to_vec_pretty(&signing_payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("signing-payload.sha256"),
        format!("{}  signing-payload.json\n", sha256_file(&signing_path)?),
    )
    .map_err(|e| e.to_string())?;
    Ok(output)
}

pub fn finalize(root: &Path, capsule: &Path, destination: &Path) -> Result<PathBuf, String> {
    if destination.exists() {
        return Err(format!(
            "final handoff destination already exists: {}",
            destination.display()
        ));
    }
    let clean = Command::new("git")
        .args(["diff", "HEAD", "--quiet", "--exit-code"])
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if !clean.success() {
        return Err("handoff generator has tracked dirt".into());
    }
    let rehearsal = crate::rehearsal::rehearse(root, capsule, &destination.join("rehearsal"))?;
    let handoff = prepare(root, capsule, &destination.join("handoff"))?;
    let stable_v1 = crate::release_profile::assess_capsule(
        root,
        "stable-v1",
        capsule,
        &destination.join("stable-v1-readiness.json"),
    )?;
    let generator_commit = command(root, "git", &["rev-parse", "HEAD"])?;
    let report = FinalHandoff {
        schema_version: 1,
        status: "pass",
        candidate_version: VERSION,
        candidate_source_sha: CANDIDATE_SHA,
        generator_commit,
        publication_performed: false,
        handoff_manifest_sha256: sha256_file(&handoff)?,
        signing_payload_sha256: sha256_file(&destination.join("handoff/signing-payload.json"))?,
        publication_rehearsal_sha256: sha256_file(&rehearsal)?,
        stable_v1_assessment_sha256: sha256_file(&stable_v1)?,
    };
    let report_path = destination.join("final-handoff.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        destination.join("final-handoff.sha256"),
        format!("{}  final-handoff.json\n", sha256_file(&report_path)?),
    )
    .map_err(|e| e.to_string())?;
    Ok(report_path)
}

fn validate_source(root: &Path) -> Result<(), String> {
    let candidate = root.join("../bevy_openpencil");
    let head = command(&candidate, "git", &["rev-parse", "HEAD"])?;
    if head != CANDIDATE_SHA {
        return Err(format!("candidate source drift: {head} != {CANDIDATE_SHA}"));
    }
    let status = Command::new("git")
        .args(["diff", "HEAD", "--quiet", "--exit-code"])
        .current_dir(&candidate)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("candidate source has tracked dirt".into());
    }
    for repository in [root, &candidate] {
        let status = Command::new("git")
            .args(["diff", "HEAD", "--quiet", "--exit-code", "--", "Cargo.lock"])
            .current_dir(repository)
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("{} has Cargo.lock drift", repository.display()));
        }
    }
    Ok(())
}

fn validate_capsule(
    capsule: &Path,
    integration_sha: &str,
) -> Result<
    (
        ArtifactManifest,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
    ),
    String,
> {
    for file in [
        "complete",
        "release-report.json",
        "release-report.md",
        "capsule.json",
        "artifact-manifest.json",
        "provenance.json",
        "sbom.spdx.json",
        "paired-clean.json",
        "reproducibility.json",
        "performance.json",
    ] {
        if !capsule.join(file).is_file() {
            return Err(format!("capsule is missing {file}"));
        }
    }
    let manifest_path = capsule.join("artifact-manifest.json");
    let manifest_hash = sha256_file(&manifest_path)?;
    if manifest_hash != MANIFEST_SHA256 {
        return Err(format!(
            "unexpected artifact manifest checksum {manifest_hash}"
        ));
    }
    let artifact_manifest: ArtifactManifest = read_json(&manifest_path)?;
    let provenance: serde_json::Value = read_json(&capsule.join("provenance.json"))?;
    validate_provenance(&provenance, integration_sha)?;
    let paired: serde_json::Value = read_json(&capsule.join("paired-clean.json"))?;
    if paired["status"] != "pass"
        || paired["matching_artifact_hashes"] != true
        || paired["first_artifact_manifest_sha256"].as_str() != Some(MANIFEST_SHA256)
        || paired["second_artifact_manifest_sha256"].as_str() != Some(MANIFEST_SHA256)
        || paired["first_run"] == paired["second_run"]
    {
        return Err("paired clean evidence is missing, stale, or mismatched".into());
    }
    let report: serde_json::Value = read_json(&capsule.join("release-report.json"))?;
    validate_report(&report)?;
    let capsule_metadata: serde_json::Value = read_json(&capsule.join("capsule.json"))?;
    validate_source_manifest(&provenance, &capsule_metadata, integration_sha)?;
    for (file, expected) in [
        (
            "artifacts/openpencil_ui_schema-0.1.0-rc.1.crate",
            SCHEMA_SHA256,
        ),
        ("artifacts/bevy_openpencil-0.1.0-rc.1.crate", RUNTIME_SHA256),
    ] {
        let actual = sha256_file(&capsule.join(file))?;
        if actual != expected {
            return Err(format!("unexpected package checksum for {file}: {actual}"));
        }
    }
    Ok((artifact_manifest, provenance, report, capsule_metadata))
}

fn validate_provenance(
    provenance: &serde_json::Value,
    integration_sha: &str,
) -> Result<(), String> {
    if provenance["artifact_manifest_sha256"].as_str() != Some(MANIFEST_SHA256) {
        return Err("provenance does not bind retained artifact manifest bytes".into());
    }
    if provenance["integration_commit"].as_str() != Some(integration_sha) {
        return Err("provenance integration commit is stale or mismatched".into());
    }
    Ok(())
}

fn validate_report(report: &serde_json::Value) -> Result<(), String> {
    if report["verdict"] != "RELEASE"
        || report["profile"]["id"] != "rc1"
        || report["profile"]["candidate_version"] != VERSION
    {
        return Err("capsule is incomplete, stale, or failed RC1 certification".into());
    }
    Ok(())
}

fn validate_source_manifest(
    provenance: &serde_json::Value,
    capsule: &serde_json::Value,
    integration_sha: &str,
) -> Result<(), String> {
    let actual = source_manifest(provenance, capsule)?;
    for (id, expected) in [
        ("integration", integration_sha),
        ("opui", OPUI_SHA),
        ("openpencil", OPENPENCIL_SHA),
        ("bevy_openpencil", CANDIDATE_SHA),
        ("openpencil/vendor/agent", AGENT_SHA),
        ("openpencil/vendor/casement", CASEMENT_SHA),
        ("openpencil/vendor/jian", JIAN_SHA),
        ("veritasium", VERITASIUM_SHA),
    ] {
        if actual.get(id).map(String::as_str) != Some(expected) {
            return Err(format!("source manifest mismatch for {id}"));
        }
    }
    Ok(())
}

fn source_manifest(
    provenance: &serde_json::Value,
    capsule: &serde_json::Value,
) -> Result<BTreeMap<String, String>, String> {
    let mut sources = BTreeMap::new();
    sources.insert(
        "integration".into(),
        provenance["integration_commit"]
            .as_str()
            .ok_or("provenance has no integration commit")?
            .into(),
    );
    for repository in capsule["repositories"]
        .as_array()
        .ok_or("capsule has no repositories")?
    {
        sources.insert(
            repository["id"]
                .as_str()
                .ok_or("repository has no id")?
                .into(),
            repository["sha"]
                .as_str()
                .ok_or("repository has no sha")?
                .into(),
        );
    }
    for submodule in capsule["submodules"]
        .as_array()
        .ok_or("capsule has no submodules")?
    {
        sources.insert(
            format!(
                "openpencil/vendor/{}",
                submodule["id"].as_str().ok_or("submodule has no id")?
            ),
            submodule["sha"]
                .as_str()
                .ok_or("submodule has no sha")?
                .into(),
        );
    }
    sources.insert("veritasium".into(), VERITASIUM_SHA.into());
    Ok(sources)
}

fn cargo_locks(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut locks = BTreeMap::new();
    for (id, path) in [
        ("integration", root.join("Cargo.lock")),
        (
            "bevy_openpencil",
            root.join("../bevy_openpencil/Cargo.lock"),
        ),
    ] {
        let contents = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let accepted = format!(
            "git+https://codeberg.org/caniko/rs-veritasium.git?rev={VERITASIUM_SHA}#{VERITASIUM_SHA}"
        );
        if !contents.contains(&accepted) {
            return Err(format!("{id} Cargo.lock lacks the accepted Veritasium pin"));
        }
        locks.insert(id.into(), sha256_file(&path)?);
    }
    Ok(locks)
}

fn evidence_summary(capsule: &Path, report: &serde_json::Value) -> Result<EvidenceSummary, String> {
    let cases = report["cases"]
        .as_array()
        .ok_or("release report has no cases")?;
    let mut accessibility_nodes = 0;
    for case in cases {
        let case_id = case["case"].as_str().ok_or("case has no id")?;
        let size = case["size"].as_str().ok_or("case has no size")?;
        let nodes: Vec<serde_json::Value> =
            read_json(&capsule.join(format!("cases/{case_id}-{size}/accessibility.json")))?;
        accessibility_nodes += nodes.len();
    }
    let stress: serde_json::Value = read_json(&capsule.join("graphical-stress/stress.json"))?;
    Ok(EvidenceSummary {
        visual_cases_passed: cases
            .iter()
            .filter(|case| case["folded"] == "RELEASE")
            .count(),
        visual_cases_total: cases.len(),
        accessibility_snapshots: cases.len(),
        accessibility_nodes,
        graphical_stress_status: stress["status"]
            .as_str()
            .ok_or("graphical stress has no status")?
            .into(),
        graphical_stress_cycles: stress["completed_cycles"]
            .as_u64()
            .ok_or("graphical stress has no completed cycle count")?,
    })
}

fn packages(manifest: &ArtifactManifest) -> Result<Vec<HandoffPackage>, String> {
    let mut packages = Vec::new();
    for (name, file, expected) in [
        (
            "openpencil_ui_schema",
            "artifacts/openpencil_ui_schema-0.1.0-rc.1.crate",
            SCHEMA_SHA256,
        ),
        (
            "bevy_openpencil",
            "artifacts/bevy_openpencil-0.1.0-rc.1.crate",
            RUNTIME_SHA256,
        ),
    ] {
        let artifact = manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.file == file)
            .ok_or_else(|| format!("artifact manifest is missing {file}"))?;
        if artifact.sha256 != expected {
            return Err(format!("artifact manifest checksum mismatch for {file}"));
        }
        packages.push(HandoffPackage {
            name: name.into(),
            version: VERSION.into(),
            file: file.into(),
            bytes: artifact.bytes,
            sha256: artifact.sha256.clone(),
        });
    }
    Ok(packages)
}

fn copy(source: PathBuf, destination: PathBuf) -> Result<(), String> {
    fs::copy(&source, &destination)
        .map(|_| ())
        .map_err(|e| format!("{} -> {}: {e}", source.display(), destination.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{}: {e}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_manifest_rejects_unknown_fields() {
        let value = r#"{
            "schema_version": 2,
            "candidate_version": "0.1.0-rc.1",
            "candidate_source_sha": "sha",
            "authoritative_capsule": "capsule",
            "artifact_manifest_sha256": "hash",
            "packages": [],
            "repositories": {},
            "cargo_locks": {},
            "evidence": {},
            "evidence_summary": {
                "visual_cases_passed": 0,
                "visual_cases_total": 0,
                "accessibility_snapshots": 0,
                "accessibility_nodes": 0,
                "graphical_stress_status": "pass",
                "graphical_stress_cycles": 0
            },
            "external_status": {},
            "publication_status": "not-published",
            "signing_status": "unsigned",
            "hosted_ci_status": "blocked",
            "unexpected": true
        }"#;
        assert!(serde_json::from_str::<HandoffManifest>(value).is_err());
    }

    #[test]
    fn package_manifest_rejects_missing_or_mismatched_packages() {
        let missing = ArtifactManifest { artifacts: vec![] };
        assert!(packages(&missing).unwrap_err().contains("missing"));
        let mismatched = ArtifactManifest {
            artifacts: vec![Artifact {
                file: "artifacts/openpencil_ui_schema-0.1.0-rc.1.crate".into(),
                sha256: "wrong".into(),
                bytes: 1,
            }],
        };
        assert!(packages(&mismatched).unwrap_err().contains("mismatch"));
    }

    #[test]
    fn stale_report_and_provenance_are_rejected() {
        let stale_report = serde_json::json!({
            "verdict": "RELEASE",
            "profile": { "id": "rc1", "candidate_version": "0.1.0-rc.0" }
        });
        assert!(
            validate_report(&stale_report)
                .unwrap_err()
                .contains("stale")
        );

        let mismatched_provenance = serde_json::json!({
            "artifact_manifest_sha256": MANIFEST_SHA256,
            "integration_commit": "wrong"
        });
        assert!(
            validate_provenance(&mismatched_provenance, "expected")
                .unwrap_err()
                .contains("mismatched")
        );
    }
}
