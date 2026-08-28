use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExpectedDiagnostic {
    pub code: String,
    pub severity: String,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub runtime_id: Option<String>,
    #[serde(default)]
    pub strategy: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaseCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CaseManifest {
    pub id: String,
    pub source: String,
    #[serde(default = "default_entrypoint")]
    pub entrypoint: String,
    pub viewports: Vec<String>,
    #[serde(default)]
    pub expected_runtime_ids: Vec<String>,
    #[serde(default)]
    pub expected_diagnostics: Vec<ExpectedDiagnostic>,
    #[serde(default)]
    pub capability_requirements: Vec<String>,
    #[serde(default)]
    pub commands: Vec<CaseCommand>,
    #[serde(default = "default_true")]
    pub visual: bool,
    #[serde(default)]
    pub raster_required: bool,
    #[serde(default)]
    pub font_required: bool,
    #[serde(default)]
    pub release_blocking: bool,
    #[serde(default)]
    pub smoke: bool,
    #[serde(default)]
    pub notes: String,
}

fn default_entrypoint() -> String {
    "default".into()
}

fn default_true() -> bool {
    true
}

impl CaseManifest {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn source_path(&self, root: &Path) -> PathBuf {
        root.join(&self.source)
    }
}

pub fn discover_cases(root: &Path) -> Result<Vec<(PathBuf, CaseManifest)>, String> {
    let dir = root.join("conformance/cases");
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut names: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .collect();
    names.sort_by_key(|e| e.file_name());
    for ent in names {
        let manifest = ent.path().join("case.toml");
        if manifest.is_file() {
            let case = CaseManifest::load(&manifest)?;
            out.push((manifest, case));
        }
    }
    Ok(out)
}
