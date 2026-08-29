use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use opui_integration::case::{CaseCommand, CaseManifest, discover_cases};
use opui_integration::cert::{self, sha256_file, sidecar_digest};
use opui_integration::computed_diff;
use opui_integration::gate::{
    CertificationVerdicts, GateResult, GateStatus, Verdict, classify_certification,
};
use opui_integration::lock::{self, LockFile, format_reports, unauthorized_dirty};
use opui_integration::package::{
    audit_package, compare_json, expected_diagnostics, load_capabilities, manifest_diagnostics,
    resolved_bevy_versions,
};
use opui_integration::release_artifacts;
use opui_integration::release_profile::ReleaseProfile;
use serde::Serialize;

fn main() {
    if let Err(e) = run() {
        eprintln!("FAIL: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: opui-certify CASE SIZE | --release")?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if first == "--release" {
        return certify_release(&root, "alpha3");
    }
    if first == "--release-profile" {
        return certify_release(&root, &args.next().ok_or("missing release profile")?);
    }
    if first == "--release-clean" {
        let result = opui_integration::clean::certify_release_clean(&root, "alpha3")?;
        println!("{}", result.evidence.display());
        if !result.aggregate_success {
            return Err(format!(
                "clean aggregate completed with a non-release verdict: {}",
                result.evidence.display()
            ));
        }
        return Ok(());
    }
    if first == "--release-clean-profile" {
        let profile = args.next().ok_or("missing release profile")?;
        let result = opui_integration::clean::certify_release_clean(&root, &profile)?;
        println!("{}", result.evidence.display());
        if !result.aggregate_success {
            return Err(format!(
                "clean aggregate completed with a non-release verdict: {}",
                result.evidence.display()
            ));
        }
        return Ok(());
    }
    if first == "--package-preflight" {
        let report = if args.next().as_deref() == Some("rc2") {
            opui_integration::package_preflight::run_rc2_package_preflight(&root)?
        } else {
            opui_integration::package_preflight::run_package_preflight(&root)?
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if first == "--rc1-artifacts" {
        let destination = PathBuf::from(args.next().ok_or("missing DESTINATION")?);
        fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
        write_sbom(
            &root.join("../bevy_openpencil"),
            &destination.join("sbom.spdx.json"),
            &[],
        )?;
        if !release_artifacts::build_and_compare(&root, &destination)? {
            return Err("independent artifact hashes differ".into());
        }
        release_artifacts::write_unsigned_provenance(&root, &destination)?;
        println!("{}", destination.display());
        return Ok(());
    }
    if first == "--rc2-artifacts" {
        let destination = PathBuf::from(args.next().ok_or("missing DESTINATION")?);
        fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
        let sbom = destination.join("sbom.spdx.json");
        write_sbom(&root.join("../bevy_openpencil"), &sbom, &[])?;
        if !release_artifacts::build_and_compare_rc2(&root, &destination)? {
            return Err("independent RC2 artifact hashes differ".into());
        }
        let archives = [
            destination.join("artifacts/openpencil_ui_schema-0.1.0-rc.2.crate"),
            destination.join("artifacts/bevy_openpencil-0.1.0-rc.2.crate"),
        ];
        write_sbom(&root.join("../bevy_openpencil"), &sbom, &archives)?;
        release_artifacts::refresh_sbom_artifact(&destination)?;
        release_artifacts::write_unsigned_provenance(&root, &destination)?;
        println!("{}", destination.display());
        return Ok(());
    }
    if first == "--snapshot-determinism" {
        let case = args.next().ok_or("missing CASE")?;
        let size = args.next().ok_or("missing SIZE")?;
        let profile = args.next().ok_or("missing debug|release")?;
        let lane = args.next().ok_or("missing LANE")?;
        let runs = args
            .next()
            .ok_or("missing RUNS")?
            .parse::<usize>()
            .map_err(|_| "RUNS must be an integer")?;
        let report = snapshot_determinism(&root, &case, &size, &profile, &lane, runs)?;
        println!("{}", report.display());
        return Ok(());
    }
    if first == "--accept-golden" {
        let case = args.next().ok_or("missing CASE")?;
        let size = args.next().ok_or("missing SIZE")?;
        let (_, provenance) = certify_one(&root, &case, &size, false)?;
        let run = provenance.parent().ok_or("missing run directory")?;
        let golden = root.join(format!("conformance/goldens/{case}/{size}"));
        fs::create_dir_all(&golden).map_err(|e| e.to_string())?;
        for (actual, expected) in [
            (
                run.join("mapping.json"),
                golden.join("mapping_snapshot.json"),
            ),
            (
                run.join("computed.json"),
                golden.join("computed_snapshot.json"),
            ),
        ] {
            print_diff(&expected, &actual);
            fs::copy(&actual, &expected).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if first == "--diagnose-visual" {
        let case = args.next().ok_or("missing CASE")?;
        let size = args.next().ok_or("missing SIZE")?;
        let (verdicts, provenance) = certify_one(&root, &case, &size, false)?;
        let run = provenance.parent().ok_or("missing run directory")?;
        if !run.join("complete").is_file() {
            return Err("fresh certification did not complete".into());
        }
        let diagnostics = run.join("node-diff.json");
        if !diagnostics.is_file() {
            return Err("fresh certification produced no node-diff.json".into());
        }
        println!("{}", diagnostics.display());
        if verdicts.case_verdict == Verdict::DoNotRelease {
            std::process::exit(1);
        }
        return Ok(());
    }
    if first == "--verify-lock" || first == "--verify-lock-strict" || first == "--verify-lock-dev" {
        let strict = first != "--verify-lock-dev";
        let lock = lock::load_lock(&root.join("repos.lock.toml"))?;
        let reports = lock::verify_lock(&root, &lock, strict);
        print!("{}", format_reports(&reports));
        if reports.iter().any(|r| !r.ok) {
            std::process::exit(1);
        }
        if !strict && reports.iter().any(|r| r.dirty) {
            println!("lock mode: development dirty; RELEASE is forbidden");
        }
        return Ok(());
    }
    let size = args.next().ok_or("usage: opui-certify CASE SIZE")?;
    let strict = std::env::var("OPUI_CERTIFY_STRICT").ok().as_deref() == Some("1");
    let (verdicts, path) = certify_one(&root, &first, &size, strict)?;
    println!("{}", fs::read_to_string(path).map_err(|e| e.to_string())?);
    if verdicts.case_verdict == Verdict::DoNotRelease {
        std::process::exit(1);
    }
    Ok(())
}

fn certify_release(root: &Path, profile_id: &str) -> Result<(), String> {
    let profile = ReleaseProfile::load(root, profile_id)?;
    let cases = discover_cases(root)?;
    if cases.is_empty() {
        return Err("no conformance/cases/*/case.toml".into());
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let rel = cert::run_root(root).join(format!("release-{ts}"));
    fs::create_dir_all(&rel).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut overall = Verdict::Release;
    let mut release_gates = Vec::new();
    let mut case_conformance = GateStatus::Pass;
    let mut environment_eligibility = GateStatus::Pass;
    let sbom_started = Instant::now();
    let sbom_path = rel.join("sbom.spdx.json");
    let sbom = write_sbom(&root.join("../bevy_openpencil"), &sbom_path, &[]);
    let mut sbom_gate = GateResult::new("sbom", true);
    sbom_gate.evidence.push(sbom_path.display().to_string());
    let sbom_gate = sbom_gate.finish(
        if sbom.is_ok() {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "cargo metadata --format-version 1 --locked",
        sbom.as_ref().map(|_| "ok").unwrap_or_else(|error| error),
        sbom_started.elapsed(),
    );
    if profile_id != "rc2" {
        release_gates.push(sbom_gate);
    }
    for command in release_commands(root, &rel, profile_id) {
        let started = Instant::now();
        let result = run_process_at(&command.cwd, command.program, &command.args)?;
        let log_path = rel.join(format!("{}.log", command.id));
        fs::write(&log_path, &result.log).map_err(|e| e.to_string())?;
        let mut gate = GateResult::new(command.id, true);
        gate.evidence.push(log_path.display().to_string());
        let gate = gate.finish(
            if result.success {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            format!("{} {}", command.program, command.args.join(" ")),
            if result.success {
                "ok"
            } else {
                "command failed"
            },
            started.elapsed(),
        );
        if matches!(profile_id, "rc1" | "rc2" | "stable-v1") && command.id == "package_preflight" {
            let mut adopter = gate.clone();
            adopter.id = "package_runtime_adopter".into();
            adopter.message = if adopter.status == GateStatus::Pass {
                "independent packaged runtime adopter loaded, mounted, and resolved main_menu.play"
                    .into()
            } else {
                "independent packaged runtime adopter failed".into()
            };
            release_gates.push(adopter);
        }
        release_gates.push(gate);
    }
    if matches!(profile_id, "rc1" | "rc2" | "stable-v1") {
        release_gates.extend(candidate_policy_gates(root, &rel, profile_id));
        release_gates.extend(candidate_artifact_gates(root, &rel, profile_id));
        release_gates.extend(external_gates(root, &profile));
    }
    for (_, case) in &cases {
        for size in &case.viewports {
            let run = rel.join(format!("cases/{}-{size}", case.id));
            fs::create_dir_all(&run).map_err(|e| e.to_string())?;
            let (verdicts, prov) = certify_one_inner(root, &case.id, size, true, &run)?;
            case_conformance = combine_status(case_conformance, verdicts.case_conformance);
            environment_eligibility =
                combine_status(environment_eligibility, verdicts.environment_eligibility);
            overall = overall.worse(verdicts.release_verdict);
            rows.push(serde_json::json!({
                "case": case.id,
                "size": size,
                "case_conformance": verdicts.case_conformance,
                "environment_eligibility": verdicts.environment_eligibility,
                "case_verdict": verdicts.case_verdict.as_str(),
                "release_verdict": verdicts.release_verdict.as_str(),
                "verdict": verdicts.release_verdict.as_str(),
                "folded": verdicts.release_verdict.as_str(),
                "provenance": prov.display().to_string(),
                "release_blocking": case.release_blocking,
            }));
        }
    }
    let packaging_readiness = fold_release_gates(
        release_gates
            .iter()
            .filter(|gate| is_packaging_gate(&gate.id)),
    );
    let command_conformance =
        fold_release_gates(release_gates.iter().filter(|gate| {
            !is_packaging_gate(&gate.id) && !profile.external_gates.contains(&gate.id)
        }));
    let technical_conformance = combine_status(command_conformance, case_conformance);
    let publication_readiness = packaging_readiness;
    let profile_evaluation = profile.evaluate(&release_gates);
    overall = overall.worse(profile_evaluation.verdict);
    let report = serde_json::json!({
        "profile": profile,
        "profile_evaluation": profile_evaluation,
        "verdict": overall.as_str(),
        "technical_conformance": technical_conformance,
        "environmental_eligibility": environment_eligibility,
        "packaging_readiness": packaging_readiness,
        "publication_readiness": publication_readiness,
        "release_gates": release_gates,
        "cases": rows,
        "run": rel.display().to_string(),
    });
    let json = serde_json::to_string_pretty(&report).unwrap();
    fs::write(rel.join("release-report.json"), &json).map_err(|e| e.to_string())?;
    let mut md = format!(
        "# OPUI release report\n\n**{}**\n\n\
         - Profile: `{profile_id}`\n\
         - Technical conformance: {technical_conformance:?}\n\
         - Environmental eligibility: {environment_eligibility:?}\n\
         - Packaging readiness: {packaging_readiness:?}\n\
         - Publication readiness: {publication_readiness:?}\n\n",
        overall.as_str()
    );
    md.push_str("## Release gates\n\n");
    for gate in &release_gates {
        md.push_str(&format!("- {}: {:?}\n", gate.id, gate.status));
    }
    if !profile_evaluation.issues.is_empty() {
        md.push_str("\n## Profile issues\n\n");
        for issue in &profile_evaluation.issues {
            md.push_str(&format!("- {issue}\n"));
        }
    }
    md.push_str("\n## Cases\n\n");
    for row in &rows {
        md.push_str(&format!(
            "- {} {} → {}\n",
            row["case"], row["size"], row["verdict"]
        ));
    }
    fs::write(rel.join("release-report.md"), md).map_err(|e| e.to_string())?;
    fs::write(rel.join("complete"), b"complete\n").map_err(|e| e.to_string())?;
    println!("{json}");
    if overall == Verdict::DoNotRelease {
        std::process::exit(1);
    }
    Ok(())
}

fn is_packaging_gate(id: &str) -> bool {
    matches!(
        id,
        "package_schema"
            | "package_runtime"
            | "publish_dry_run_schema"
            | "package_preflight"
            | "sbom"
    )
}

fn candidate_artifact_gates(root: &Path, release: &Path, profile_id: &str) -> Vec<GateResult> {
    let started = Instant::now();
    let reproducible = if profile_id == "rc2" {
        release_artifacts::build_and_compare_rc2(root, release)
    } else {
        release_artifacts::build_and_compare(root, release)
    };
    let archive_sbom = if profile_id == "rc2" {
        match &reproducible {
            Ok(_) => {
                let archives = [
                    release.join("artifacts/openpencil_ui_schema-0.1.0-rc.2.crate"),
                    release.join("artifacts/bevy_openpencil-0.1.0-rc.2.crate"),
                ];
                write_sbom(
                    &root.join("../bevy_openpencil"),
                    &release.join("sbom.spdx.json"),
                    &archives,
                )
                .and_then(|_| release_artifacts::refresh_sbom_artifact(release))
            }
            Err(error) => Err(format!("archive build failed before SBOM rewrite: {error}")),
        }
    } else {
        Ok(())
    };
    let artifacts_built = reproducible.is_ok() && archive_sbom.is_ok();
    let hashes_match = matches!(reproducible.as_ref(), Ok(true));
    let mut artifact = GateResult::new("artifact_manifest", true);
    artifact
        .evidence
        .push(release.join("artifact-manifest.json").display().to_string());
    let artifact = artifact.finish(
        if artifacts_built {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "build and hash distributable crate archives and SBOM",
        reproducible
            .as_ref()
            .map_err(String::as_str)
            .and(archive_sbom.as_ref().map_err(String::as_str))
            .map(|_| "artifact manifest generated")
            .unwrap_or_else(|error| error),
        started.elapsed(),
    );
    let mut reproducibility = GateResult::new("reproducibility", true);
    reproducibility
        .evidence
        .push(release.join("reproducibility.json").display().to_string());
    let reproducibility = reproducibility.finish(
        if hashes_match {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "build release archives in two independent target directories and compare SHA-256",
        if hashes_match {
            "independent artifact hashes match"
        } else {
            "independent artifact hashes differ or build failed"
        },
        started.elapsed(),
    );

    let started = Instant::now();
    let provenance_result = release_artifacts::write_unsigned_provenance(root, release);
    let mut provenance = GateResult::new("unsigned_provenance", true);
    provenance
        .evidence
        .push(release.join("provenance.json").display().to_string());
    let provenance = provenance.finish(
        if provenance_result.is_ok() {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "write unsigned provenance bound to repository, toolchain, and artifact hashes",
        provenance_result
            .as_ref()
            .map(|_| "unsigned provenance generated; signing remains external")
            .unwrap_or_else(|error| error),
        started.elapsed(),
    );
    let mut gates = Vec::new();
    if profile_id == "rc2" {
        let mut sbom = GateResult::new("sbom", true);
        sbom.evidence
            .push(release.join("sbom.spdx.json").display().to_string());
        gates.push(
            sbom.finish(
                if archive_sbom.is_ok() {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                "write SPDX SBOM with direct archive subjects",
                archive_sbom
                    .as_ref()
                    .map(|_| "archive-bearing SBOM generated")
                    .unwrap_or_else(|error| error),
                started.elapsed(),
            ),
        );
    }
    gates.extend([artifact, reproducibility, provenance]);
    gates
}

fn external_gates(root: &Path, profile: &ReleaseProfile) -> Vec<GateResult> {
    let source = root.join(format!("external-results/{}.toml", profile.id));
    if !source.is_file() {
        return profile
            .external_gates
            .iter()
            .map(|id| {
                let mut gate = GateResult::new(id, true);
                if id == "public_source_closure" {
                    gate.evidence
                        .push("https://github.com/caniko/bevy_openpencil".into());
                }
                gate.finish(
                    GateStatus::Blocked,
                    "import authorized external result",
                    "no external result has been imported for this candidate",
                    Duration::ZERO,
                )
            })
            .collect();
    }
    match opui_integration::external_results::load_gates(root, profile) {
        Ok(gates) => gates,
        Err(error) => vec![GateResult::new("external_results", true).finish(
            GateStatus::Fail,
            "validate external result schema",
            error,
            Duration::ZERO,
        )],
    }
}

fn candidate_policy_gates(root: &Path, release: &Path, profile_id: &str) -> Vec<GateResult> {
    let started = Instant::now();
    let callback_rfc = root.join("docs/lifecycle-callback-rfc.md");
    let callback_text = fs::read_to_string(&callback_rfc).unwrap_or_default();
    let callback_ok = [
        "not part of OPUI v1",
        "must not use them as an implicit callback protocol",
        "versioned contract",
        "does not block RC1",
    ]
    .iter()
    .all(|required| callback_text.contains(required));
    let mut callback = GateResult::new("callback_scope", true);
    callback.evidence.push(callback_rfc.display().to_string());
    if callback_rfc.is_file()
        && let Ok(hash) = sha256_file(&callback_rfc)
    {
        callback.output_hashes.insert("rfc".into(), hash);
    }
    let callback = callback.finish(
        if callback_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "verify tracked callback deferral RFC",
        if callback_ok {
            "callbacks deferred to a future versioned contract"
        } else {
            "callback deferral RFC is missing required scope boundaries"
        },
        started.elapsed(),
    );

    let started = Instant::now();
    let expected = "7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0";
    let lock_paths = [
        root.join("Cargo.lock"),
        root.join("../bevy_openpencil/Cargo.lock"),
    ];
    let mut pin_ok = true;
    let mut sources = BTreeMap::new();
    let expected_source =
        format!("git+https://codeberg.org/caniko/rs-veritasium.git?rev={expected}#{expected}");
    for path in &lock_paths {
        match bevy_sources(path) {
            Ok(found) if all_bevy_sources_match(&found, &expected_source) => {
                sources.insert(path.display().to_string(), found.join("\n"));
            }
            Ok(found) => {
                pin_ok = false;
                sources.insert(path.display().to_string(), found.join("\n"));
            }
            Err(error) => {
                pin_ok = false;
                sources.insert(path.display().to_string(), error);
            }
        }
    }
    let mut pin = GateResult::new("veritasium_pin", true);
    pin.evidence = lock_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    pin.output_hashes = sources;
    let pin = pin.finish(
        if pin_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "verify all Bevy Cargo.lock sources use the accepted Veritasium revision",
        if pin_ok {
            "accepted exact Veritasium revision"
        } else {
            "Bevy source does not resolve exclusively to the accepted revision"
        },
        started.elapsed(),
    );
    let _ = fs::write(
        release.join(format!("{profile_id}-policy.json")),
        serde_json::to_vec_pretty(&[&callback, &pin]).unwrap(),
    );
    vec![callback, pin]
}

fn bevy_sources(lock: &Path) -> Result<Vec<String>, String> {
    let value: toml::Value = toml::from_str(&fs::read_to_string(lock).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut sources = value["package"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|package| {
            package["name"]
                .as_str()
                .is_some_and(|name| name == "bevy" || name.starts_with("bevy_"))
                && package["version"].as_str() == Some("0.19.1")
        })
        .map(|package| {
            package
                .get("source")
                .and_then(toml::Value::as_str)
                .unwrap_or("<missing source>")
                .to_owned()
        })
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    Ok(sources)
}

fn all_bevy_sources_match(sources: &[String], expected: &str) -> bool {
    !sources.is_empty() && sources.iter().all(|source| source == expected)
}

fn fold_release_gates<'a>(gates: impl Iterator<Item = &'a GateResult>) -> GateStatus {
    gates.fold(GateStatus::Pass, |status, gate| {
        combine_status(status, gate.status)
    })
}

fn combine_status(left: GateStatus, right: GateStatus) -> GateStatus {
    match (left, right) {
        (GateStatus::Fail, _) | (_, GateStatus::Fail) => GateStatus::Fail,
        (GateStatus::Blocked, _) | (_, GateStatus::Blocked) => GateStatus::Blocked,
        (GateStatus::Skipped, _) | (_, GateStatus::Skipped) => GateStatus::Skipped,
        _ => GateStatus::Pass,
    }
}

fn certify_one(
    root: &Path,
    case_id: &str,
    size: &str,
    release_strict: bool,
) -> Result<(CertificationVerdicts, PathBuf), String> {
    let run = cert::new_run_dir(root, case_id, size)?;
    match certify_one_inner(root, case_id, size, release_strict, &run) {
        Ok(result) => Ok(result),
        Err(error) => {
            let verdicts = CertificationVerdicts {
                case_conformance: GateStatus::Fail,
                environment_eligibility: GateStatus::Blocked,
                case_verdict: Verdict::DoNotRelease,
                release_verdict: Verdict::DoNotRelease,
            };
            let report = serde_json::json!({
                "case": case_id,
                "size": size,
                "run": run.display().to_string(),
                "gates": [{
                    "id": "certification_internal",
                    "status": "fail",
                    "required": true,
                    "message": error,
                }],
                "case_conformance": verdicts.case_conformance,
                "environment_eligibility": verdicts.environment_eligibility,
                "case_verdict": verdicts.case_verdict.as_str(),
                "release_verdict": verdicts.release_verdict.as_str(),
                "verdict": verdicts.release_verdict.as_str(),
            });
            let path = run.join("provenance.json");
            let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
            fs::write(&path, &bytes).map_err(|e| e.to_string())?;
            fs::write(run.join("case-report.json"), bytes).map_err(|e| e.to_string())?;
            Ok((verdicts, path))
        }
    }
}

fn certify_one_inner(
    root: &Path,
    case_id: &str,
    size: &str,
    release_strict: bool,
    run: &Path,
) -> Result<(CertificationVerdicts, PathBuf), String> {
    let started = Instant::now();
    let (w, h) = parse_size(size)?;
    let (manifest_path, case) = load_case(root, case_id)?;
    let src = case.source_path(root);
    let retained_manifest = run.join("case.toml");
    let retained_source = run.join("source.op");
    fs::copy(&manifest_path, &retained_manifest).map_err(|e| e.to_string())?;
    fs::copy(&src, &retained_source).map_err(|e| e.to_string())?;
    let source_sha = sha256_file(&src)?;
    fs::write(run.join("source.sha256"), format!("{source_sha}\n")).map_err(|e| e.to_string())?;
    let mut gates = Vec::new();
    let mut runner = std::collections::BTreeMap::new();
    runner.insert("host".into(), hostname());
    runner.insert("cwd".into(), root.display().to_string());

    let mut g = GateResult::new("source_exists", true);
    g.runner = runner.clone();
    let t = Instant::now();
    if src.is_file() {
        gates.push(g.finish(
            GateStatus::Pass,
            format!("stat {}", src.display()),
            "ok",
            t.elapsed(),
        ));
    } else {
        gates.push(g.finish(
            GateStatus::Fail,
            format!("stat {}", src.display()),
            "missing source",
            t.elapsed(),
        ));
    }

    let lock = lock::load_lock(&root.join("repos.lock.toml"))?;
    let reports = lock::verify_lock(root, &lock, release_strict);
    let dirty = unauthorized_dirty(&reports);
    let lock_ok = reports.iter().all(|r| r.ok || (r.dirty && r.allowed_dirt));
    let mut g = GateResult::new("repository_lock", true);
    g.evidence.push(format_reports(&reports));
    gates.push(g.finish(
        if lock_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "verify-lock",
        format_reports(&reports),
        started.elapsed(),
    ));
    let g = GateResult::new("clean_repository_state", true);
    gates.push(g.finish(
        if dirty {
            GateStatus::Fail
        } else if reports.iter().any(|r| r.dirty) {
            GateStatus::Blocked
        } else {
            GateStatus::Pass
        },
        "git status",
        if dirty {
            "unauthorized dirty"
        } else {
            "ok or allowed dirt"
        },
        started.elapsed(),
    ));

    let pkg_name = format!("{}.opui", case.id);
    let pkg = run.join(&pkg_name);
    let mut g = GateResult::new("deterministic_export", true);
    let t = Instant::now();
    match export(root, &src, &pkg) {
        Ok(()) => {
            let h1 = sha256_file(&pkg)?;
            let copy_dir = run.join("determinism-copy");
            fs::create_dir_all(&copy_dir).map_err(|e| e.to_string())?;
            let copy = copy_dir.join(&pkg_name);
            export(root, &src, &copy)?;
            let h2 = sha256_file(&copy)?;
            let sidecar1 = sidecar_digest(&pkg.with_file_name(format!("{pkg_name}.assets")))?;
            let sidecar2 = sidecar_digest(&copy.with_file_name(format!("{pkg_name}.assets")))?;
            g.output_hashes.insert("package".into(), h1.clone());
            g.output_hashes.insert("sidecar".into(), sidecar1.clone());
            if h1 == h2 && sidecar1 == sidecar2 {
                gates.push(g.finish(GateStatus::Pass, "op export", h1, t.elapsed()));
            } else {
                gates.push(g.finish(
                    GateStatus::Fail,
                    "op export",
                    format!(
                        "nondeterministic package {h1} vs {h2}; sidecar {sidecar1} vs {sidecar2}"
                    ),
                    t.elapsed(),
                ));
            }
        }
        Err(e) => gates.push(g.finish(GateStatus::Fail, "op export", e, t.elapsed())),
    }

    let assets = pkg.with_file_name(format!("{pkg_name}.assets"));
    let assets_sha = sidecar_digest(&assets)?;
    let required_kinds = [case.font_required.then_some("font")]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let package_audit = audit_package(&pkg, &required_kinds)?;
    fs::write(
        run.join("checker-result.json"),
        serde_json::to_vec_pretty(&package_audit).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let mut g = GateResult::new("checker", true);
    g.evidence
        .push(run.join("checker-result.json").display().to_string());
    gates.push(
        g.finish(
            if package_audit
                .checker_diagnostics
                .iter()
                .all(|d| d.severity != "error")
            {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            "opui::check_path",
            format!("{} diagnostic(s)", package_audit.checker_diagnostics.len()),
            started.elapsed(),
        ),
    );
    let mut g = GateResult::new("sidecar_dependencies", !required_kinds.is_empty());
    g.output_hashes.insert("sidecar".into(), assets_sha.clone());
    g.evidence
        .push(run.join("checker-result.json").display().to_string());
    gates.push(g.finish(
        if required_kinds.is_empty() {
            GateStatus::Skipped
        } else if package_audit.ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "semantic sidecar audit",
        format!(
            "assets={} missing_kinds={:?} orphans={:?}",
            package_audit.assets.len(),
            package_audit.missing_required_kinds,
            package_audit.orphan_files
        ),
        started.elapsed(),
    ));

    let raster = if case.raster_required {
        let mut g = GateResult::new("raster_export", true);
        let t = Instant::now();
        match try_raster(root, &src, run, &case.id) {
            Ok(h) => {
                let raster_pkg = run.join(format!("{}-raster.opui", case.id));
                let audit = audit_package(&raster_pkg, &["raster_fallback"])?;
                let manifest: serde_json::Value =
                    serde_json::from_slice(&fs::read(&raster_pkg).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                let fallback_nodes = manifest["nodes"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(_, node)| node["type"] == "fallback")
                    .count();
                fs::write(
                    run.join("raster-audit.json"),
                    serde_json::to_vec_pretty(&audit).unwrap(),
                )
                .map_err(|e| e.to_string())?;
                g.output_hashes.insert("raster".into(), h.clone());
                gates.push(g.finish(
                    if audit.ok && fallback_nodes > 0 {
                        GateStatus::Pass
                    } else {
                        GateStatus::Fail
                    },
                    "op export --raster-native",
                    format!(
                        "sha256={h} fallback_nodes={fallback_nodes} audit_ok={}",
                        audit.ok
                    ),
                    t.elapsed(),
                ));
                Some(h)
            }
            Err(e) => {
                gates.push(g.finish(
                    GateStatus::Fail,
                    "op export --raster-native",
                    e,
                    t.elapsed(),
                ));
                None
            }
        }
    } else {
        let g = GateResult::new("raster_export", false);
        gates.push(g.finish(
            GateStatus::Skipped,
            "",
            "not required",
            Instant::now().elapsed(),
        ));
        None
    };
    let runtime_pkg_name = if raster.is_some() {
        format!("{}-raster.opui", case.id)
    } else {
        pkg_name.clone()
    };

    let (reference, ref_meta) = if case.visual {
        generate_reference(&lock, &src, run, w, h)
    } else {
        (
            None,
            RefMeta {
                command: lock.reference_renderer.path.clone(),
                message: "not required".into(),
                hash_ok: false,
            },
        )
    };
    let mut g = GateResult::new("reference_generation", case.visual);
    if !case.visual {
        gates.push(g.finish(
            GateStatus::Skipped,
            ref_meta.command.clone(),
            ref_meta.message.clone(),
            Instant::now().elapsed(),
        ));
    } else if let Some(ref h) = reference {
        g.output_hashes.insert("reference".into(), h.clone());
        gates.push(g.finish(
            GateStatus::Pass,
            ref_meta.command.clone(),
            h.clone(),
            Instant::now().elapsed(),
        ));
    } else {
        gates.push(g.finish(
            GateStatus::Blocked,
            ref_meta.command.clone(),
            ref_meta.message.clone(),
            Instant::now().elapsed(),
        ));
    }
    let mut g = GateResult::new("reference_provenance", case.visual);
    g.input_hashes
        .insert("renderer".into(), lock.reference_renderer.sha256.clone());
    gates.push(g.finish(
        if !case.visual {
            GateStatus::Skipped
        } else if reference.is_some() && ref_meta.hash_ok {
            GateStatus::Pass
        } else {
            GateStatus::Blocked
        },
        lock.reference_renderer.path.clone(),
        ref_meta.message.clone(),
        Instant::now().elapsed(),
    ));

    let control = case
        .visual
        .then(|| run_shot(root, run, size, true, &pkg_name, None, &case.entrypoint))
        .transpose()?;
    let g = GateResult::new("control_oracle", case.visual);
    gates.push(
        g.finish(
            if !case.visual {
                GateStatus::Skipped
            } else if control.as_ref().is_some_and(|shot| shot.success) {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            "bevy-shot --scene control",
            control
                .as_ref()
                .map(|shot| shot.log.clone())
                .unwrap_or_else(|| "not required".into()),
            Instant::now().elapsed(),
        ),
    );

    let ref_arg = (case.visual && reference.is_some()).then(|| run.join("reference.png"));
    let opui = case
        .visual
        .then(|| {
            run_shot(
                root,
                run,
                size,
                false,
                &runtime_pkg_name,
                ref_arg.as_deref(),
                &case.entrypoint,
            )
        })
        .transpose()?;
    let g = GateResult::new("runtime_capture", case.visual);
    gates.push(
        g.finish(
            if !case.visual {
                GateStatus::Skipped
            } else if opui.as_ref().is_some_and(|shot| shot.success) {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            "bevy-shot --scene opui",
            opui.as_ref()
                .map(|shot| shot.log.clone())
                .unwrap_or_else(|| "not required".into()),
            Instant::now().elapsed(),
        ),
    );
    let g = GateResult::new("visual_compare", case.visual);
    let visual_status = if !case.visual {
        GateStatus::Skipped
    } else if reference.is_none() {
        GateStatus::Blocked
    } else if opui.as_ref().is_some_and(|shot| shot.visual_pass) {
        GateStatus::Pass
    } else {
        GateStatus::Fail
    };
    gates.push(
        g.finish(
            visual_status,
            "image_metrics",
            opui.as_ref()
                .map(|shot| shot.log.clone())
                .unwrap_or_else(|| "not required".into()),
            Instant::now().elapsed(),
        ),
    );

    let probe = run_runtime_probe(
        root,
        run,
        &runtime_pkg_name,
        &case.entrypoint,
        size,
        &case.expected_runtime_ids,
    )?;
    fs::write(run.join("runtime-probe.log"), &probe.log).map_err(|e| e.to_string())?;
    let loader_path = run.join("loader-probe.json");
    let loader =
        read_json(&loader_path).unwrap_or_else(|error| serde_json::json!({"error": error}));
    let dependencies_loaded = loader["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .all(|d| d["load"] == "Loaded" && d["recursive_dependencies"] == "Loaded");
    let g = GateResult::new("asset_server_load", true);
    gates.push(g.finish(
        if probe.success
            && loader["loaded_with_dependencies"] == true
            && loader["error"].is_null()
            && dependencies_loaded
        {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "runtime-probe",
        probe.log.clone(),
        Instant::now().elapsed(),
    ));

    let node_diff = if case.visual && reference.is_some() {
        opui_integration::visual_diagnostics::write_node_diff(
            root,
            run,
            &run.join(&runtime_pkg_name),
        )
    } else {
        Err("visual diagnostics not required".into())
    };
    let mut g = GateResult::new("node_visual_attribution", case.visual);
    if let Ok(path) = &node_diff {
        g.evidence.push(path.display().to_string());
    }
    gates.push(
        g.finish(
            if !case.visual {
                GateStatus::Skipped
            } else if node_diff.is_ok() {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            "node-level image diagnostics",
            node_diff
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|error| error.clone()),
            Instant::now().elapsed(),
        ),
    );

    let package_ids_ok = check_runtime_ids(&pkg, &case.expected_runtime_ids);
    let registry_path = run.join("runtime-registry.json");
    let registry = read_json(&registry_path).unwrap_or_else(|error| {
        serde_json::json!({"expected": case.expected_runtime_ids, "found": [], "missing": case.expected_runtime_ids, "error": error})
    });
    let registry_ids_ok = registry["missing"].as_array().is_some_and(Vec::is_empty);
    let g = GateResult::new("package_runtime_ids", !case.expected_runtime_ids.is_empty());
    gates.push(g.finish(
        if case.expected_runtime_ids.is_empty() {
            GateStatus::Skipped
        } else if package_ids_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "package runtime_id set",
        format!("{:?}", case.expected_runtime_ids),
        Instant::now().elapsed(),
    ));
    let g = GateResult::new(
        "runtime_registry_ids",
        !case.expected_runtime_ids.is_empty(),
    );
    gates.push(g.finish(
        if case.expected_runtime_ids.is_empty() {
            GateStatus::Skipped
        } else if registry_ids_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "OpenPencilRuntimeIds after reconciliation",
        registry_path.display().to_string(),
        Instant::now().elapsed(),
    ));

    let golden_root = root.join(format!("conformance/goldens/{}/{size}", case.id));
    let computed_diff_changes = computed_diff::write_report(computed_diff::ComputedDiffInputs {
        actual: &run.join("computed.json"),
        golden: &golden_root.join("computed_snapshot.json"),
        mapping: &run.join("mapping.json"),
        mapping_golden: &golden_root.join("mapping_snapshot.json"),
        case_manifest: &manifest_path,
        source_manifest: &src,
        context: &run.join("computed-context.json"),
        output_dir: run,
    })?;
    for (id, actual) in [
        ("mapping_snapshot", run.join("mapping.json")),
        ("computed_snapshot", run.join("computed.json")),
    ] {
        let golden = golden_root.join(format!("{id}.json"));
        let comparison = compare_json(&actual, &golden);
        let mut g = GateResult::new(id, true);
        if id == "computed_snapshot" {
            g.evidence
                .push(run.join("computed-diff.json").display().to_string());
            g.evidence
                .push(run.join("computed-diff.md").display().to_string());
        }
        let status = if comparison.is_ok() {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        };
        let message = match comparison {
            Ok(()) => golden.display().to_string(),
            Err(error) if id == "computed_snapshot" => {
                format!("{error}; structured changes={computed_diff_changes}")
            }
            Err(error) => error,
        };
        gates.push(g.finish(
            status,
            "structured JSON equality",
            message,
            Instant::now().elapsed(),
        ));
    }

    let manifest: serde_json::Value = read_json(&pkg)?;
    let actual_diagnostics = manifest_diagnostics(&manifest);
    let expected_diagnostics = expected_diagnostics(&case.expected_diagnostics);
    let diagnostics_ok = actual_diagnostics == expected_diagnostics;
    let g = GateResult::new("expected_diagnostics", true);
    gates.push(g.finish(
        if diagnostics_ok {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "exact diagnostic tuple comparison",
        format!("expected={expected_diagnostics:?} actual={actual_diagnostics:?}"),
        Instant::now().elapsed(),
    ));

    let capabilities =
        load_capabilities(&root.join("conformance/capabilities/bevy-openpencil.toml"))?;
    let missing_capabilities = case
        .capability_requirements
        .iter()
        .filter(|required| !capabilities.contains(*required))
        .cloned()
        .collect::<Vec<_>>();
    let g = GateResult::new(
        "capability_requirements",
        !case.capability_requirements.is_empty(),
    );
    gates.push(g.finish(
        if case.capability_requirements.is_empty() {
            GateStatus::Skipped
        } else if missing_capabilities.is_empty() {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "bevy-openpencil capability profile",
        format!("missing={missing_capabilities:?}"),
        Instant::now().elapsed(),
    ));

    let commands = run_case_commands(root, run, &case.commands)?;
    let g = GateResult::new("case_commands", !case.commands.is_empty());
    gates.push(g.finish(
        if case.commands.is_empty() {
            GateStatus::Skipped
        } else if commands.iter().all(|result| result.success) {
            GateStatus::Pass
        } else {
            GateStatus::Fail
        },
        "declared case commands",
        format!("{} command(s)", commands.len()),
        Instant::now().elapsed(),
    ));

    let verdicts = classify_certification(&gates, dirty && release_strict, case.release_blocking);

    let pkg_sha = sha256_file(&pkg).unwrap_or_default();
    let bevy_versions = resolved_bevy_versions(&root.join("Cargo.lock"))?;
    let prov = serde_json::json!({
        "case": case.id,
        "size": size,
        "width": w,
        "height": h,
        "run": run.display().to_string(),
        "manifest": retained_manifest.display().to_string(),
        "source": retained_source.display().to_string(),
        "source_sha256": source_sha,
        "package": pkg_name,
        "package_sha256": pkg_sha,
        "sidecar_sha256": assets_sha,
        "policy": opui_integration::image_metrics::POLICY_ID,
        "reference": reference,
        "renderer": {
            "path": lock.reference_renderer.path,
            "sha256": lock.reference_renderer.sha256,
            "note": lock.reference_renderer.note,
            "command": ref_meta.command,
        },
        "raster": raster,
        "smoke": case.smoke,
        "release_blocking": case.release_blocking,
        "case_conformance": verdicts.case_conformance,
        "environment_eligibility": verdicts.environment_eligibility,
        "case_verdict": verdicts.case_verdict.as_str(),
        "release_verdict": verdicts.release_verdict.as_str(),
        "lock": reports,
        "runner": runner,
        "bevy_versions": bevy_versions,
        "case_commands": commands,
        "node_diff": node_diff.ok().map(|path| path.display().to_string()),
        "gates": gates,
        "verdict": verdicts.release_verdict.as_str(),
    });
    let text = serde_json::to_string_pretty(&prov).unwrap();
    let dest = run.join("provenance.json");
    fs::write(&dest, &text).map_err(|e| e.to_string())?;
    fs::write(run.join("case-report.json"), &text).map_err(|e| e.to_string())?;
    fs::write(run.join("complete"), b"complete\n").map_err(|e| e.to_string())?;
    write_latest_pointer(root, &case.id, size, run)?;
    Ok((verdicts, dest))
}

fn load_case(root: &Path, id: &str) -> Result<(PathBuf, CaseManifest), String> {
    let path = root.join(format!("conformance/cases/{id}/case.toml"));
    if path.is_file() {
        return Ok((path.clone(), CaseManifest::load(&path)?));
    }
    Err(format!("missing {}", path.display()))
}

fn parse_size(size: &str) -> Result<(u32, u32), String> {
    let (w, h) = size.split_once('x').ok_or("SIZE must be WxH")?;
    Ok((
        w.parse().map_err(|_| "bad width")?,
        h.parse().map_err(|_| "bad height")?,
    ))
}

fn export(root: &Path, src: &Path, dest: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(["run", "--quiet", "--manifest-path"])
        .arg(root.join("../openpencil/Cargo.toml"))
        .args(["-p", "op-cli", "--", "export", "--file"])
        .arg(src)
        .args(["--format", "opui", "--output"])
        .arg(dest)
        .args(["--item", "artboard"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("export failed".into());
    }
    Ok(())
}

fn try_raster(root: &Path, src: &Path, run: &Path, case: &str) -> Result<String, String> {
    let dest = run.join(format!("{case}-raster.opui"));
    if let Some(executable) = std::env::var_os("OPUI_RASTER_EXPORTER") {
        let status = Command::new(executable)
            .args(["export", "--file"])
            .arg(src)
            .args(["--format", "opui", "--output"])
            .arg(&dest)
            .args(["--item", "artboard", "--raster-native"])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("raster export failed".into());
        }
        return sha256_file(&dest);
    }
    let openpencil = fs::canonicalize(root.join("../openpencil")).map_err(|e| e.to_string())?;
    let status = Command::new("nix")
        .args(["run"])
        .arg(format!("path:{}#op-cli-raster", openpencil.display()))
        .args(["--", "export", "--file"])
        .arg(src)
        .args(["--format", "opui", "--output"])
        .arg(&dest)
        .args(["--item", "artboard", "--raster-native"])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("raster export failed".into());
    }
    sha256_file(&dest)
}

struct RefMeta {
    command: String,
    message: String,
    hash_ok: bool,
}

fn generate_reference(
    lock: &LockFile,
    src: &Path,
    run: &Path,
    width: u32,
    height: u32,
) -> (Option<String>, RefMeta) {
    let exe = Path::new(&lock.reference_renderer.path);
    let mut meta = RefMeta {
        command: format!("{} --render-shots", exe.display()),
        message: String::new(),
        hash_ok: false,
    };
    if !exe.is_file() {
        meta.message = "pinned renderer missing".into();
        return (None, meta);
    }
    match sha256_file(exe) {
        Ok(h) if h == lock.reference_renderer.sha256 => meta.hash_ok = true,
        Ok(h) => {
            meta.message = format!("renderer hash {h} != pin");
            return (None, meta);
        }
        Err(e) => {
            meta.message = e;
            return (None, meta);
        }
    }
    let status = Command::new(exe)
        .args(["--render-shots"])
        .arg(src)
        .arg(run)
        .arg("1")
        .arg(width.to_string())
        .arg(height.to_string())
        .env("OPENPENCIL_RENDER_MARGIN", "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(_) => {
            meta.message = "render-shots failed".into();
            return (None, meta);
        }
        Err(e) => {
            meta.message = e.to_string();
            return (None, meta);
        }
    }
    let png = ["artboard.png", "reference.png"]
        .into_iter()
        .map(|n| run.join(n))
        .find(|p| p.is_file());
    let Some(png) = png else {
        meta.message = "no reference png in this run".into();
        return (None, meta);
    };
    let dest = run.join("reference.png");
    if png != dest {
        let _ = fs::copy(&png, &dest);
    }
    match sha256_file(&dest) {
        Ok(h) => {
            meta.message = h.clone();
            (Some(h), meta)
        }
        Err(e) => {
            meta.message = e;
            (None, meta)
        }
    }
}

#[derive(serde::Serialize)]
struct ShotResult {
    success: bool,
    visual_pass: bool,
    log: String,
}

#[derive(serde::Serialize)]
struct ProcessResult {
    program: String,
    args: Vec<String>,
    success: bool,
    log: String,
}

struct ReleaseCommand {
    id: &'static str,
    cwd: PathBuf,
    program: &'static str,
    args: Vec<String>,
}

fn release_commands(root: &Path, release: &Path, profile_id: &str) -> Vec<ReleaseCommand> {
    let command = |id, cwd: PathBuf, program, args: &[&str]| ReleaseCommand {
        id,
        cwd,
        program,
        args: args.iter().map(|s| s.to_string()).collect(),
    };
    let bevy = root.join("../bevy_openpencil");
    let opui = root.join("../opui");
    let openpencil = root.join("../openpencil");
    let package_preflight_args: &[&str] = if profile_id == "rc2" {
        &[
            "run",
            "--quiet",
            "--bin",
            "opui-certify",
            "--",
            "--package-preflight",
            "rc2",
        ]
    } else {
        &[
            "run",
            "--quiet",
            "--bin",
            "opui-certify",
            "--",
            "--package-preflight",
        ]
    };
    let mut commands = vec![
        command(
            "format_integration",
            root.into(),
            "cargo",
            &["fmt", "--all", "--check"],
        ),
        command(
            "format_runtime",
            bevy.clone(),
            "cargo",
            &["fmt", "--all", "--check"],
        ),
        command(
            "format_checker",
            opui.clone(),
            "cargo",
            &["fmt", "--all", "--check"],
        ),
        command(
            "format_exporter",
            openpencil.clone(),
            "cargo",
            &["fmt", "-p", "op-cli", "-p", "op-runtime-ui", "--check"],
        ),
        command(
            "clippy_integration",
            root.into(),
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        command(
            "clippy_runtime",
            bevy.clone(),
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        command("audit_runtime", bevy.clone(), "cargo", &["audit"]),
        command(
            "licenses_runtime",
            bevy.clone(),
            "cargo",
            &["deny", "check", "licenses", "sources"],
        ),
        command("export_fixture", root.into(), "just", &["export"]),
        command(
            "tests_integration",
            root.into(),
            "cargo",
            &["test", "--workspace", "--offline"],
        ),
        command(
            "tests_runtime",
            bevy.clone(),
            "cargo",
            &["test", "--workspace", "--offline"],
        ),
        command(
            "tests_checker",
            opui.clone(),
            "cargo",
            &["test", "--all-targets", "--offline"],
        ),
        command(
            "tests_exporter",
            openpencil.clone(),
            "cargo",
            &["test", "-p", "op-runtime-ui", "--offline", "--lib"],
        ),
        command(
            "all_targets_integration",
            root.into(),
            "cargo",
            &[
                "check",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--offline",
            ],
        ),
        command(
            "all_targets_runtime",
            bevy.clone(),
            "cargo",
            &[
                "check",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--offline",
            ],
        ),
        command(
            "examples_runtime",
            bevy.clone(),
            "cargo",
            &["check", "--workspace", "--examples", "--offline"],
        ),
        command(
            "feature_off_runtime",
            bevy.clone(),
            "cargo",
            &[
                "check",
                "-p",
                "bevy_openpencil",
                "--no-default-features",
                "--offline",
            ],
        ),
        command(
            "feature_off_integration",
            root.into(),
            "cargo",
            &[
                "check",
                "--no-default-features",
                "--all-targets",
                "--offline",
            ],
        ),
        command(
            "hot_reload_mutations",
            root.into(),
            "cargo",
            &[
                "test",
                "--offline",
                "--test",
                "runtime",
                "--",
                "--nocapture",
            ],
        ),
        command(
            "negative_exports",
            root.into(),
            "cargo",
            &["test", "--offline", "--test", "negative"],
        ),
        command("clean_reproduction", root.into(), "just", &["repro-clean"]),
        command(
            "package_schema",
            bevy.clone(),
            "cargo",
            &["package", "--list", "-p", "openpencil_ui_schema"],
        ),
        command(
            "package_runtime",
            bevy.clone(),
            "cargo",
            &["package", "--list", "-p", "bevy_openpencil"],
        ),
        command(
            "publish_dry_run_schema",
            bevy.clone(),
            "cargo",
            &["publish", "--dry-run", "-p", "openpencil_ui_schema"],
        ),
        command(
            "package_preflight",
            root.into(),
            "cargo",
            package_preflight_args,
        ),
    ];
    if matches!(profile_id, "rc1" | "rc2" | "stable-v1") {
        let baseline = "36388fb7be76f464e32586493e78c070960f9fa5..HEAD";
        let baseline_rev = "36388fb7be76f464e32586493e78c070960f9fa5";
        for (id, package) in [
            ("public_api_schema", "openpencil_ui_schema"),
            ("public_api_runtime", "bevy_openpencil"),
        ] {
            commands.push(command(
                id,
                bevy.clone(),
                "cargo",
                &[
                    "public-api",
                    "--manifest-path",
                    "Cargo.toml",
                    "-p",
                    package,
                    "diff",
                    "--deny",
                    "all",
                    baseline,
                ],
            ));
        }
        for (id, package) in [
            ("semver_schema", "openpencil_ui_schema"),
            ("semver_runtime", "bevy_openpencil"),
        ] {
            commands.push(command(
                id,
                bevy.clone(),
                "cargo",
                &[
                    "semver-checks",
                    "--manifest-path",
                    "Cargo.toml",
                    "--baseline-rev",
                    baseline_rev,
                    "--release-type",
                    "patch",
                    "-p",
                    package,
                ],
            ));
        }
        commands.push(ReleaseCommand {
            id: "graphical_stress",
            cwd: root.into(),
            program: "nix",
            args: vec![
                "--option".into(),
                "min-free".into(),
                "0".into(),
                "--option".into(),
                "max-free".into(),
                "0".into(),
                "develop".into(),
                ".#graphical".into(),
                "--command".into(),
                "just".into(),
                "graphical-rc1-stress-test".into(),
                release.join("graphical-stress").display().to_string(),
            ],
        });
        commands.push(ReleaseCommand {
            id: "performance",
            cwd: root.into(),
            program: "cargo",
            args: vec![
                "run".into(),
                "--quiet".into(),
                "--release".into(),
                "--features".into(),
                "visual".into(),
                "--bin".into(),
                "perf-probe".into(),
                "--".into(),
                release.join("performance.json").display().to_string(),
            ],
        });
    }
    commands.push(ReleaseCommand {
        id: "graphical_showcase",
        cwd: root.into(),
        program: "nix",
        args: vec![
            "--option".into(),
            "min-free".into(),
            "0".into(),
            "--option".into(),
            "max-free".into(),
            "0".into(),
            "develop".into(),
            ".#graphical".into(),
            "--command".into(),
            "just".into(),
            "graphical-showcase-test".into(),
            release.join("graphical").display().to_string(),
        ],
    });
    commands
}

fn write_sbom(workspace: &Path, destination: &Path, archives: &[PathBuf]) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    let mut ids = std::collections::HashMap::new();
    let mut spdx_packages = Vec::with_capacity(packages.len());
    for (index, package) in packages.iter().enumerate() {
        let id = package["id"].as_str().ok_or("package has no id")?;
        let spdx_id = format!("SPDXRef-Package-{index}");
        ids.insert(id.to_string(), spdx_id.clone());
        let name = package["name"].as_str().ok_or("package has no name")?;
        let version = package["version"]
            .as_str()
            .ok_or("package has no version")?;
        let source = package["source"].as_str().unwrap_or("NOASSERTION");
        let license = package["license"].as_str().unwrap_or("NOASSERTION");
        spdx_packages.push(serde_json::json!({
            "SPDXID": spdx_id,
            "name": name,
            "versionInfo": version,
            "downloadLocation": source,
            "filesAnalyzed": false,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": license,
            "supplier": "NOASSERTION",
            "externalRefs": [{
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": format!("pkg:cargo/{name}@{version}"),
            }],
        }));
    }
    let mut archives = archives.to_vec();
    archives.sort();
    let mut relationships = Vec::new();
    for (index, archive) in archives.iter().enumerate() {
        let name = archive
            .file_name()
            .ok_or("archive has no filename")?
            .to_string_lossy();
        let relative = archive
            .strip_prefix(destination.parent().ok_or("SBOM has no parent")?)
            .map_err(|_| format!("archive {} is outside the SBOM root", archive.display()))?
            .to_string_lossy();
        let archive_id = format!("SPDXRef-Archive-{index}");
        spdx_packages.push(serde_json::json!({
            "SPDXID": &archive_id,
            "name": name,
            "downloadLocation": format!("file:{relative}"),
            "filesAnalyzed": false,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "MIT",
            "supplier": "NOASSERTION",
            "checksums": [{
                "algorithm": "SHA256",
                "checksumValue": sha256_file(archive)?,
            }],
        }));
        relationships.push(serde_json::json!({
            "spdxElementId": "SPDXRef-DOCUMENT",
            "relationshipType": "DESCRIBES",
            "relatedSpdxElement": archive_id,
        }));
    }
    for node in metadata["resolve"]["nodes"]
        .as_array()
        .ok_or("cargo metadata has no resolved nodes")?
    {
        let Some(from) = node["id"].as_str().and_then(|id| ids.get(id)) else {
            continue;
        };
        for dependency in node["dependencies"].as_array().into_iter().flatten() {
            let Some(to) = dependency.as_str().and_then(|id| ids.get(id)) else {
                continue;
            };
            relationships.push(serde_json::json!({
                "spdxElementId": from,
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": to,
            }));
        }
    }
    let identity = cert::sha256_bytes(
        &serde_json::to_vec(&(&spdx_packages, &relationships))
            .map_err(|error| error.to_string())?,
    );
    let document = serde_json::json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": "bevy_openpencil dependency SBOM",
        "documentNamespace": format!("https://github.com/caniko/bevy_openpencil/sbom/{identity}"),
        "creationInfo": {
            "created": "2026-08-27T00:00:00Z",
            "creators": ["Tool: opui-certify"],
        },
        "packages": spdx_packages,
        "relationships": relationships,
        "annotations": [{
            "annotationType": "OTHER",
            "annotator": "Tool: opui-certify",
            "annotationDate": "2026-08-27T00:00:00Z",
            "comment": "Proprietary dependency disposition: none declared. Vulnerability disposition is recorded by the cargo-audit release gate.",
        }],
    });
    fs::write(
        destination,
        serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn run_runtime_probe(
    root: &Path,
    dir: &Path,
    package: &str,
    entrypoint: &str,
    size: &str,
    expected_ids: &[String],
) -> Result<ProcessResult, String> {
    run_runtime_probe_profile(root, dir, package, entrypoint, size, expected_ids, false)
}

fn run_runtime_probe_profile(
    root: &Path,
    dir: &Path,
    package: &str,
    entrypoint: &str,
    size: &str,
    expected_ids: &[String],
    release: bool,
) -> Result<ProcessResult, String> {
    let mut args = vec!["run".into(), "--quiet".into()];
    if release {
        args.push("--release".into());
    }
    args.extend([
        "--features".into(),
        "visual".into(),
        "--bin".into(),
        "runtime-probe".into(),
        "--".into(),
        dir.display().to_string(),
        package.into(),
        entrypoint.into(),
        size.into(),
    ]);
    args.extend(expected_ids.iter().cloned());
    run_process(root, "cargo", &args)
}

#[derive(Serialize)]
struct SnapshotRepeat {
    run: usize,
    computed_sha256: String,
    mapping_sha256: String,
    context_sha256: String,
    matches_golden: bool,
}

#[derive(Serialize)]
struct SnapshotDeterminismReport {
    case: String,
    size: String,
    profile: String,
    lane: String,
    renders: bool,
    runs: Vec<SnapshotRepeat>,
    computed_sha256: String,
    mapping_sha256: String,
    context_sha256: String,
    golden_sha256: String,
    all_actual_equal: bool,
}

fn snapshot_determinism(
    root: &Path,
    case_id: &str,
    size: &str,
    profile: &str,
    lane: &str,
    runs: usize,
) -> Result<PathBuf, String> {
    if runs < 20 {
        return Err("snapshot determinism requires at least 20 runs".into());
    }
    let release = match profile {
        "debug" => false,
        "release" => true,
        _ => return Err("profile must be debug or release".into()),
    };
    let (_, case) = load_case(root, case_id)?;
    let (_, provenance) = certify_one(root, case_id, size, false)?;
    let base = provenance
        .parent()
        .ok_or("certification has no run directory")?;
    let package_name = format!("{case_id}.opui");
    let package = base.join(&package_name);
    if !package.is_file() {
        return Err(format!("missing base package {}", package.display()));
    }
    let assets = package.with_file_name(format!("{package_name}.assets"));
    let repeat_root = base.join(format!("snapshot-determinism-{lane}-{profile}"));
    fs::create_dir(&repeat_root).map_err(|error| error.to_string())?;
    let golden = root.join(format!(
        "conformance/goldens/{case_id}/{size}/computed_snapshot.json"
    ));
    let golden_sha256 = sha256_file(&golden)?;
    let mut repeated = Vec::with_capacity(runs);
    for run in 0..runs {
        let dir = repeat_root.join(format!("run-{run:02}"));
        fs::create_dir(&dir).map_err(|error| error.to_string())?;
        fs::copy(&package, dir.join(&package_name)).map_err(|error| error.to_string())?;
        if assets.is_dir() {
            copy_dir(&assets, &dir.join(format!("{package_name}.assets")))?;
        }
        let probe = run_runtime_probe_profile(
            root,
            &dir,
            &package_name,
            &case.entrypoint,
            size,
            &case.expected_runtime_ids,
            release,
        )?;
        fs::write(dir.join("runtime-probe.log"), &probe.log).map_err(|error| error.to_string())?;
        if !probe.success {
            return Err(format!("runtime probe {run} failed: {}", probe.log));
        }
        let computed_sha256 = sha256_file(&dir.join("computed.json"))?;
        repeated.push(SnapshotRepeat {
            run,
            mapping_sha256: sha256_file(&dir.join("mapping.json"))?,
            context_sha256: sha256_file(&dir.join("computed-context.json"))?,
            matches_golden: computed_sha256 == golden_sha256,
            computed_sha256,
        });
    }
    let first = repeated.first().ok_or("no repeated snapshots")?;
    let all_actual_equal = repeated_snapshots_equal(&repeated);
    let report = SnapshotDeterminismReport {
        case: case_id.into(),
        size: size.into(),
        profile: profile.into(),
        lane: lane.into(),
        renders: false,
        computed_sha256: first.computed_sha256.clone(),
        mapping_sha256: first.mapping_sha256.clone(),
        context_sha256: first.context_sha256.clone(),
        golden_sha256,
        runs: repeated,
        all_actual_equal,
    };
    let path = repeat_root.join("snapshot-determinism.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if !all_actual_equal {
        return Err(format!("snapshot nondeterminism: {}", path.display()));
    }
    Ok(path)
}

fn repeated_snapshots_equal(runs: &[SnapshotRepeat]) -> bool {
    runs.first().is_some_and(|first| {
        runs.iter().all(|run| {
            run.computed_sha256 == first.computed_sha256
                && run.mapping_sha256 == first.mapping_sha256
                && run.context_sha256 == first.context_sha256
        })
    })
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(from, to).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn run_case_commands(
    root: &Path,
    run: &Path,
    commands: &[CaseCommand],
) -> Result<Vec<ProcessResult>, String> {
    let mut results = Vec::new();
    for command in commands {
        results.push(run_process(root, &command.program, &command.args)?);
    }
    fs::write(
        run.join("case-commands.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(results)
}

fn run_process(root: &Path, program: &str, args: &[String]) -> Result<ProcessResult, String> {
    run_process_at(root, program, args)
}

fn run_process_at(root: &Path, program: &str, args: &[String]) -> Result<ProcessResult, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    Ok(ProcessResult {
        program: program.into(),
        args: args.to_vec(),
        success: output.status.success(),
        log: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    })
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn print_diff(expected: &Path, actual: &Path) {
    if !expected.exists() {
        println!(
            "new golden {} from {}",
            expected.display(),
            actual.display()
        );
        return;
    }
    if let Ok(output) = Command::new("diff")
        .args(["-u"])
        .arg(expected)
        .arg(actual)
        .output()
    {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
}

fn run_shot(
    root: &Path,
    dir: &Path,
    size: &str,
    control: bool,
    package: &str,
    reference: Option<&Path>,
    entrypoint: &str,
) -> Result<ShotResult, String> {
    let scene = if control { "control" } else { "opui" };
    let mut cmd = Command::new("cargo");
    cmd.args([
        "run",
        "--quiet",
        "--features",
        "visual",
        "--bin",
        "bevy-shot",
        "--",
        "--scene",
        scene,
        "--package",
        package,
        "--entrypoint",
        entrypoint,
    ]);
    if let Some(r) = reference {
        cmd.arg("--reference").arg(r);
    }
    let out = cmd
        .arg(dir)
        .arg(size)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let success = out.status.success() && !log.contains("FAIL");
    Ok(ShotResult {
        success,
        visual_pass: success && (control || reference.is_some()),
        log,
    })
}

fn check_runtime_ids(pkg: &Path, expected: &[String]) -> bool {
    let Ok(bytes) = fs::read(pkg) else {
        return false;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let ids = opui_integration::runtime_ids(&v);
    expected.iter().all(|e| ids.iter().any(|i| i == e))
}

fn write_latest_pointer(root: &Path, case: &str, size: &str, run: &Path) -> Result<(), String> {
    if !run.join("complete").is_file() {
        return Err("refusing latest pointer for incomplete run".into());
    }
    let dir = root.join("runs");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let latest = dir.join(format!("latest-{case}-{size}"));
    let pending = dir.join(format!(".latest-{case}-{size}-{}", std::process::id()));
    fs::write(&pending, format!("{}\n", run.display())).map_err(|e| e.to_string())?;
    fs::rename(pending, latest).map_err(|e| e.to_string())
}

fn hostname() -> String {
    Command::new("hostnamectl")
        .arg("--static")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod snapshot_determinism_tests {
    use super::*;

    #[test]
    fn any_semantic_snapshot_drift_fails_repetition() {
        let run = |computed: &str| SnapshotRepeat {
            run: 0,
            computed_sha256: computed.into(),
            mapping_sha256: "mapping".into(),
            context_sha256: "context".into(),
            matches_golden: false,
        };
        assert!(repeated_snapshots_equal(&[run("same"), run("same")]));
        assert!(!repeated_snapshots_equal(&[run("first"), run("changed")]));
    }

    #[test]
    fn sbom_is_reproducible_and_contains_both_crates() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bevy_openpencil");
        let root = std::env::temp_dir().join(format!("opui-sbom-{}", std::process::id()));
        let first = root.with_extension("first.json");
        let second = root.with_extension("second.json");
        write_sbom(&workspace, &first, &[]).unwrap();
        write_sbom(&workspace, &second, &[]).unwrap();
        let first_bytes = fs::read(&first).unwrap();
        assert_eq!(first_bytes, fs::read(&second).unwrap());
        let document: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        let names: Vec<&str> = document["packages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|package| package["name"].as_str())
            .collect();
        assert!(names.contains(&"bevy_openpencil"));
        assert!(names.contains(&"openpencil_ui_schema"));
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
    }

    #[test]
    fn sbom_contains_direct_archive_subjects() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bevy_openpencil");
        let root = std::env::temp_dir().join(format!("opui-sbom-archives-{}", std::process::id()));
        fs::create_dir_all(root.join("artifacts")).unwrap();
        let archive = root.join("artifacts/bevy_openpencil-0.1.0-rc.2.crate");
        fs::write(&archive, b"archive").unwrap();
        let destination = root.join("sbom.spdx.json");
        write_sbom(&workspace, &destination, std::slice::from_ref(&archive)).unwrap();
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&destination).unwrap()).unwrap();
        let subject = document["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "bevy_openpencil-0.1.0-rc.2.crate")
            .unwrap();
        assert_eq!(
            subject["checksums"][0]["checksumValue"],
            sha256_file(&archive).unwrap()
        );
        assert!(
            document["relationships"]
                .as_array()
                .unwrap()
                .iter()
                .any(|relationship| {
                    relationship["relationshipType"] == "DESCRIBES"
                        && relationship["relatedSpdxElement"] == subject["SPDXID"]
                })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn all_bevy_sources_use_the_accepted_revision() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for lock in [
            root.join("Cargo.lock"),
            root.join("../bevy_openpencil/Cargo.lock"),
        ] {
            let sources = bevy_sources(&lock).unwrap();
            let sha = "7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0";
            let expected =
                format!("git+https://codeberg.org/caniko/rs-veritasium.git?rev={sha}#{sha}");
            assert!(all_bevy_sources_match(&sources, &expected));
        }
    }

    #[test]
    fn bevy_source_check_rejects_registry_packages() {
        let lock = std::env::temp_dir().join(format!("opui-bevy-lock-{}", std::process::id()));
        fs::write(
            &lock,
            r#"version = 4

[[package]]
name = "bevy_future"
version = "0.19.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
        )
        .unwrap();
        assert_eq!(
            bevy_sources(&lock).unwrap(),
            ["registry+https://github.com/rust-lang/crates.io-index"]
        );
        assert!(!all_bevy_sources_match(
            &bevy_sources(&lock).unwrap(),
            "canonical"
        ));
        let _ = fs::remove_file(lock);
    }

    #[test]
    fn missing_external_results_are_explicitly_blocked() {
        let profile = ReleaseProfile {
            id: "missing".into(),
            candidate_version: "0.1.0-rc.2".into(),
            required_gates: vec!["owned".into()],
            external_gates: vec!["public_source_closure".into()],
            allow_blocked_external: true,
        };
        let gates = external_gates(Path::new("/definitely/missing"), &profile);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].status, GateStatus::Blocked);
    }
}
