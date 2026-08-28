use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize)]
pub struct AssetAudit {
    pub id: String,
    pub kind: String,
    pub uri: String,
    pub sha256: String,
    pub byte_length: u64,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub valid: bool,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageAudit {
    pub assets: Vec<AssetAudit>,
    pub checker_diagnostics: Vec<DiagnosticTuple>,
    pub declared_kinds: BTreeSet<String>,
    pub required_kinds: BTreeSet<String>,
    pub missing_required_kinds: BTreeSet<String>,
    pub orphan_files: Vec<String>,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiagnosticTuple {
    pub code: String,
    pub severity: String,
    pub node_id: Option<String>,
    pub runtime_id: Option<String>,
    pub strategy: Option<String>,
}

pub fn manifest_diagnostics(manifest: &Value) -> Vec<DiagnosticTuple> {
    let mut out: Vec<_> = manifest["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| DiagnosticTuple {
            code: d["code"].as_str().unwrap_or_default().to_string(),
            severity: d["severity"].as_str().unwrap_or_default().to_string(),
            node_id: d["node_id"].as_str().map(str::to_string),
            runtime_id: d["runtime_id"].as_str().map(str::to_string),
            strategy: d["strategy"].as_str().map(str::to_string),
        })
        .collect();
    out.sort();
    out
}

pub fn audit_package(path: &Path, required_kinds: &[&str]) -> Result<PackageAudit, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let checker_diagnostics = opui::check_path(path, &opui::CheckOptions::for_path(path))
        .into_iter()
        .map(|d| DiagnosticTuple {
            code: d.code,
            severity: d.severity,
            node_id: d.node_id,
            runtime_id: d.runtime_id,
            strategy: d.strategy,
        })
        .collect::<Vec<_>>();
    let root = opui::asset_root_for(path);
    let mut declared_files = BTreeSet::new();
    let mut declared_kinds = BTreeSet::new();
    let mut assets = Vec::new();
    for (id, asset) in manifest["assets"].as_object().into_iter().flatten() {
        let kind = asset["kind"].as_str().unwrap_or_default().to_string();
        let uri = asset["uri"].as_str().unwrap_or_default().to_string();
        let sha256 = asset["sha256"].as_str().unwrap_or_default().to_string();
        let byte_length = asset["byte_length"].as_u64().unwrap_or_default();
        let mime_type = asset["mime_type"].as_str().unwrap_or_default().to_string();
        let file = root.join(&uri);
        declared_files.insert(uri.clone());
        declared_kinds.insert(kind.clone());
        assets.push(audit_asset(
            id,
            &kind,
            &uri,
            &sha256,
            byte_length,
            &mime_type,
            &file,
        ));
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    let required_kinds: BTreeSet<_> = required_kinds.iter().map(|s| s.to_string()).collect();
    let missing_required_kinds = required_kinds
        .difference(&declared_kinds)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut orphan_files = list_files(&root)?;
    orphan_files.retain(|p| !declared_files.contains(p));
    let ok = checker_diagnostics.iter().all(|d| d.severity != "error")
        && assets.iter().all(|a| a.valid)
        && missing_required_kinds.is_empty();
    Ok(PackageAudit {
        assets,
        checker_diagnostics,
        declared_kinds,
        required_kinds,
        missing_required_kinds,
        orphan_files,
        ok,
    })
}

#[allow(clippy::too_many_arguments)]
fn audit_asset(
    id: &str,
    kind: &str,
    uri: &str,
    expected_sha: &str,
    expected_len: u64,
    mime: &str,
    file: &Path,
) -> AssetAudit {
    let mut errors = Vec::new();
    let safe = !uri.is_empty()
        && !uri.starts_with('/')
        && !uri.contains("..")
        && !uri.contains('\\')
        && !uri.contains("://");
    if !safe {
        errors.push("unsafe package-relative URI".into());
    }
    let bytes = match fs::read(file) {
        Ok(bytes) if safe => bytes,
        Ok(_) => Vec::new(),
        Err(e) => {
            errors.push(format!("sidecar read: {e}"));
            Vec::new()
        }
    };
    if bytes.len() as u64 != expected_len {
        errors.push(format!("byte_length {} != {expected_len}", bytes.len()));
    }
    let actual_sha = format!("{:x}", Sha256::digest(&bytes));
    if actual_sha != expected_sha {
        errors.push(format!("sha256 {actual_sha} != {expected_sha}"));
    }
    let ext = Path::new(uri)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let expected_mime = match ext {
        "png" => Some("image/png"),
        "jpg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "woff2" => Some("font/woff2"),
        _ => None,
    };
    if expected_mime != Some(mime) {
        errors.push(format!("extension .{ext} does not match {mime}"));
    }
    let mut dimensions = (None, None);
    if matches!(kind, "image" | "raster_fallback") && !bytes.is_empty() {
        match image::load_from_memory(&bytes) {
            Ok(image) => dimensions = (Some(image.width()), Some(image.height())),
            Err(e) => errors.push(format!("image decode: {e}")),
        }
    }
    if kind == "font" && !bytes.is_empty() && ttf_parser::Face::parse(&bytes, 0).is_err() {
        errors.push("font parse failed".into());
    }
    AssetAudit {
        id: id.into(),
        kind: kind.into(),
        uri: uri.into(),
        sha256: expected_sha.into(),
        byte_length: expected_len,
        mime_type: mime.into(),
        width: dimensions.0,
        height: dimensions.1,
        valid: errors.is_empty(),
        errors,
    }
}

fn list_files(root: &Path) -> Result<Vec<String>, String> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(
                    path.strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn load_capabilities(path: &Path) -> Result<BTreeSet<String>, String> {
    #[derive(serde::Deserialize)]
    struct Profile {
        supported: Vec<String>,
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let profile: Profile = toml::from_str(&text).map_err(|e| e.to_string())?;
    Ok(profile.supported.into_iter().collect())
}

pub fn compare_json(actual: &Path, expected: &Path) -> Result<(), String> {
    let parse = |path: &Path| -> Result<Value, String> {
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    };
    let actual_value = parse(actual)?;
    let expected_value = parse(expected)?;
    if actual_value == expected_value {
        Ok(())
    } else {
        Err(format!(
            "{} differs from {}",
            actual.display(),
            expected.display()
        ))
    }
}

pub fn expected_diagnostics(values: &[crate::case::ExpectedDiagnostic]) -> Vec<DiagnosticTuple> {
    let mut out = values
        .iter()
        .map(|d| DiagnosticTuple {
            code: d.code.clone(),
            severity: d.severity.clone(),
            node_id: d.node_id.clone(),
            runtime_id: d.runtime_id.clone(),
            strategy: d.strategy.clone(),
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

pub fn resolved_bevy_versions(lock: &Path) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let text = fs::read_to_string(lock).map_err(|e| e.to_string())?;
    let value: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in value["package"].as_array().into_iter().flatten() {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        if (name == "bevy" || name.starts_with("bevy_"))
            && let Some(version) = package["version"].as_str()
        {
            out.entry(name.into()).or_default().insert(version.into());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_missing_asset_fails_directly() {
        let a = audit_asset(
            "image-x",
            "image",
            "../x.png",
            "0",
            1,
            "image/png",
            Path::new("/missing"),
        );
        assert!(!a.valid);
        assert!(a.errors.iter().any(|e| e.contains("unsafe")));
        assert!(a.errors.iter().any(|e| e.contains("sidecar")));
    }

    #[test]
    fn exact_diagnostics_are_order_independent() {
        let manifest = serde_json::json!({"diagnostics": [
            {"code":"b","severity":"warning","node_id":null,"runtime_id":null,"strategy":"none"},
            {"code":"a","severity":"info","node_id":"n","runtime_id":"r","strategy":"native"}
        ]});
        let got = manifest_diagnostics(&manifest);
        assert_eq!(got[0].code, "a");
        assert_eq!(got[1].code, "b");
    }
}
