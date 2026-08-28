use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::cert::sha256_file;

const VERSION: &str = "0.1.0-rc.1";
const RC2_VERSION: &str = "0.1.0-rc.2";

#[derive(Debug, Serialize)]
pub struct PackagePreflight {
    pub schema_package: String,
    pub schema_sha256: String,
    pub runtime_package: String,
    pub runtime_sha256: String,
    pub schema_publish_dry_run: bool,
    pub packaged_schema_resolution: bool,
    pub runtime_has_path_dependencies: bool,
    pub downstream_consumer: bool,
    pub runtime_adopter: bool,
    pub status: &'static str,
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn run_package_preflight(root: &Path) -> Result<PackagePreflight, String> {
    let workspace = root.join("../bevy_openpencil");
    let temp = TempRoot(unique_temp_root()?);

    let schema_list = cargo(
        &workspace,
        ["package", "--list", "-p", "openpencil_ui_schema"],
    )?;
    require_success("schema package list", &schema_list)?;
    let runtime_list = cargo(&workspace, ["package", "--list", "-p", "bevy_openpencil"])?;
    require_success("runtime package list", &runtime_list)?;

    let schema_dry_run = cargo(
        &workspace,
        [
            "publish",
            "--dry-run",
            "--locked",
            "-p",
            "openpencil_ui_schema",
            "--target-dir",
            temp.0.join("schema-dry-run").to_str().unwrap(),
        ],
    )?;
    require_success("schema publish dry-run", &schema_dry_run)?;

    let schema_target = temp.0.join("schema-package");
    let schema_package = cargo(
        &workspace,
        [
            "package",
            "--locked",
            "--no-verify",
            "-p",
            "openpencil_ui_schema",
            "--target-dir",
            schema_target.to_str().unwrap(),
        ],
    )?;
    require_success("schema package", &schema_package)?;
    let schema_crate = crate_file(&schema_target, "openpencil_ui_schema")?;
    let schema_unpack = temp.0.join("schema-unpacked");
    unpack(&schema_crate, &schema_unpack)?;
    let packaged_schema = schema_unpack.join(format!("openpencil_ui_schema-{VERSION}"));
    if !packaged_schema.join("Cargo.toml").is_file() {
        return Err("packaged schema archive has no Cargo.toml".into());
    }

    let runtime_workspace = temp.0.join("runtime-workspace");
    prepare_runtime_workspace(&workspace, &runtime_workspace)?;
    let runtime_target = temp.0.join("runtime-package");
    let patch = format!(
        "patch.crates-io.openpencil_ui_schema.path=\"{}\"",
        packaged_schema.display()
    );
    let runtime_package = cargo_vec(
        &runtime_workspace,
        vec![
            "--config".into(),
            patch.clone(),
            "package".into(),
            "--locked".into(),
            "--allow-dirty".into(),
            "-p".into(),
            "bevy_openpencil".into(),
            "--target-dir".into(),
            runtime_target.display().to_string(),
        ],
    )?;
    require_success("runtime package", &runtime_package)?;
    let runtime_crate = crate_file(&runtime_target, "bevy_openpencil")?;
    let runtime_unpack = temp.0.join("runtime-unpacked");
    unpack(&runtime_crate, &runtime_unpack)?;
    let runtime_root = runtime_unpack.join(format!("bevy_openpencil-{VERSION}"));
    let generated_manifest =
        fs::read_to_string(runtime_root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    validate_runtime_manifest(&generated_manifest)?;
    let runtime_validation = cargo_vec(
        &runtime_root,
        vec![
            "--config".into(),
            patch,
            "check".into(),
            "--lib".into(),
            "--target-dir".into(),
            temp.0.join("runtime-validation").display().to_string(),
        ],
    )?;
    require_success(
        "runtime validation against packaged schema",
        &runtime_validation,
    )?;
    check_downstream_consumer(&temp.0, &runtime_root, &packaged_schema)?;

    Ok(PackagePreflight {
        schema_package: schema_crate.display().to_string(),
        schema_sha256: sha256_file(&schema_crate)?,
        runtime_package: runtime_crate.display().to_string(),
        runtime_sha256: sha256_file(&runtime_crate)?,
        schema_publish_dry_run: true,
        packaged_schema_resolution: true,
        runtime_has_path_dependencies: false,
        downstream_consumer: true,
        runtime_adopter: true,
        status: "pass",
    })
}

pub fn build_package_archives(root: &Path, destination: &Path) -> Result<Vec<PathBuf>, String> {
    let workspace = root.join("../bevy_openpencil");
    let temp = TempRoot(unique_temp_root()?);
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;

    let schema_target = temp.0.join("schema-package");
    let output = cargo(
        &workspace,
        [
            "package",
            "--locked",
            "--no-verify",
            "-p",
            "openpencil_ui_schema",
            "--target-dir",
            schema_target.to_str().unwrap(),
        ],
    )?;
    require_success("schema release package", &output)?;
    let schema_crate = crate_file(&schema_target, "openpencil_ui_schema")?;
    let schema_unpack = temp.0.join("schema-unpacked");
    unpack(&schema_crate, &schema_unpack)?;
    let packaged_schema = schema_unpack.join(format!("openpencil_ui_schema-{VERSION}"));

    let runtime_workspace = temp.0.join("runtime-workspace");
    prepare_runtime_workspace(&workspace, &runtime_workspace)?;
    let runtime_target = temp.0.join("runtime-package");
    let output = cargo_vec(
        &runtime_workspace,
        vec![
            "--config".into(),
            format!(
                "patch.crates-io.openpencil_ui_schema.path=\"{}\"",
                packaged_schema.display()
            ),
            "package".into(),
            "--locked".into(),
            "--allow-dirty".into(),
            "--no-verify".into(),
            "-p".into(),
            "bevy_openpencil".into(),
            "--target-dir".into(),
            runtime_target.display().to_string(),
        ],
    )?;
    require_success("runtime release package", &output)?;
    let runtime_crate = crate_file(&runtime_target, "bevy_openpencil")?;

    [schema_crate, runtime_crate]
        .into_iter()
        .map(|source| {
            let destination =
                destination.join(source.file_name().ok_or("package has no filename")?);
            fs::copy(source, &destination).map_err(|e| e.to_string())?;
            Ok(destination)
        })
        .collect()
}

pub fn run_rc2_package_preflight(root: &Path) -> Result<PackagePreflight, String> {
    let workspace = root.join("../bevy_openpencil");
    let packaged = package_rc2(root)?;
    require_success(
        "rc2 schema package list",
        &cargo(
            &workspace,
            ["package", "--list", "-p", "openpencil_ui_schema"],
        )?,
    )?;
    require_success(
        "rc2 runtime package list",
        &cargo(&workspace, ["package", "--list", "-p", "bevy_openpencil"])?,
    )?;
    require_success(
        "rc2 schema publish dry-run",
        &cargo_vec(
            &workspace,
            vec![
                "publish".into(),
                "--dry-run".into(),
                "--locked".into(),
                "-p".into(),
                "openpencil_ui_schema".into(),
                "--target-dir".into(),
                packaged.temp.0.join("schema-dry-run").display().to_string(),
            ],
        )?,
    )?;
    let schema_unpack = packaged.temp.0.join("schema-unpacked");
    unpack(&packaged.schema_crate, &schema_unpack)?;
    let packaged_schema = schema_unpack.join(format!("openpencil_ui_schema-{RC2_VERSION}"));
    let runtime_unpack = packaged.temp.0.join("runtime-unpacked");
    unpack(&packaged.runtime_crate, &runtime_unpack)?;
    let runtime_root = runtime_unpack.join(format!("bevy_openpencil-{RC2_VERSION}"));
    let generated_manifest =
        fs::read_to_string(runtime_root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    validate_runtime_manifest_version(&generated_manifest, RC2_VERSION)?;
    check_downstream_consumer(&packaged.temp.0, &runtime_root, &packaged_schema)?;
    Ok(PackagePreflight {
        schema_package: packaged.schema_crate.display().to_string(),
        schema_sha256: sha256_file(&packaged.schema_crate)?,
        runtime_package: packaged.runtime_crate.display().to_string(),
        runtime_sha256: sha256_file(&packaged.runtime_crate)?,
        schema_publish_dry_run: true,
        packaged_schema_resolution: true,
        runtime_has_path_dependencies: false,
        downstream_consumer: true,
        runtime_adopter: true,
        status: "pass",
    })
}

pub fn build_rc2_package_archives(root: &Path, destination: &Path) -> Result<Vec<PathBuf>, String> {
    let packaged = package_rc2(root)?;
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    [&packaged.schema_crate, &packaged.runtime_crate]
        .into_iter()
        .map(|source| {
            let destination =
                destination.join(source.file_name().ok_or("package has no filename")?);
            fs::copy(source, &destination).map_err(|e| e.to_string())?;
            Ok(destination)
        })
        .collect()
}

struct Rc2Packaged {
    schema_crate: PathBuf,
    runtime_crate: PathBuf,
    temp: TempRoot,
}

fn package_rc2(root: &Path) -> Result<Rc2Packaged, String> {
    rc2_stable_rust_version()?;
    let workspace = root.join("../bevy_openpencil");
    let temp = TempRoot(unique_temp_root()?);
    let target = temp.0.join("workspace-package");
    let package = cargo_vec(
        &workspace,
        vec![
            "package".into(),
            "--workspace".into(),
            "--locked".into(),
            "--no-verify".into(),
            "--target-dir".into(),
            target.display().to_string(),
        ],
    )?;
    require_success("rc2 workspace package", &package)?;
    let schema_crate = crate_file(&target, "openpencil_ui_schema")?;
    let runtime_crate = crate_file(&target, "bevy_openpencil")?;
    Ok(Rc2Packaged {
        schema_crate,
        runtime_crate,
        temp,
    })
}

fn check_downstream_consumer(temp: &Path, runtime: &Path, schema: &Path) -> Result<(), String> {
    let consumer = temp.join("downstream-consumer");
    fs::create_dir(&consumer).map_err(|e| e.to_string())?;
    fs::create_dir(consumer.join("src")).map_err(|e| e.to_string())?;
    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            "[package]\nname = \"opui-downstream-proof\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nbevy = {{ version = \"=0.19.1\", default-features = false, features = [\"std\", \"bevy_asset\", \"bevy_ui\"] }}\nbevy_openpencil = {{ path = {:?} }}\n\n[patch.crates-io]\nbevy = {{ git = \"https://codeberg.org/caniko/rs-veritasium.git\", rev = \"7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0\" }}\nopenpencil_ui_schema = {{ path = {:?} }}\n",
            runtime, schema
        ),
    )
    .map_err(|e| e.to_string())?;
    fs::create_dir(consumer.join("assets")).map_err(|e| e.to_string())?;
    fs::copy(
        runtime.join("assets/ui/main_menu.opui"),
        consumer.join("assets/main_menu.opui"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        consumer.join("src/main.rs"),
        r#"use bevy::asset::LoadState;
use bevy::prelude::*;
use bevy_openpencil::{OpenPencilRuntimeIds, OpenPencilUi, OpenPencilUiPlugin, OpenPencilUiRoot};

fn main() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
            watch_for_changes_override: Some(false),
            ..default()
        },
        OpenPencilUiPlugin,
    ));
    let handle: Handle<OpenPencilUi> = app.world().resource::<AssetServer>().load("main_menu.opui");
    for _ in 0..80 {
        app.update();
        match app.world().resource::<AssetServer>().get_load_state(&handle) {
            Some(LoadState::Loaded) => break,
            Some(LoadState::Failed(error)) => panic!("package load failed: {error}"),
            _ => {}
        }
    }
    assert!(app.world().resource::<Assets<OpenPencilUi>>().get(&handle).is_some());
    let root = app.world_mut().spawn((Node::default(), OpenPencilUiRoot::new(handle, "main_menu"))).id();
    app.update();
    assert!(app.world().resource::<OpenPencilRuntimeIds>().get(root, "main_menu.play").is_some());
}
"#,
    )
    .map_err(|e| e.to_string())?;
    let output = cargo_vec(
        &consumer,
        vec![
            "run".into(),
            "--quiet".into(),
            "--target-dir".into(),
            temp.join("downstream-target").display().to_string(),
        ],
    )?;
    require_success("fresh packaged runtime adopter", &output)?;
    validate_consumer_bevy_pin(&consumer.join("Cargo.lock"))
}

fn validate_consumer_bevy_pin(lock: &Path) -> Result<(), String> {
    let value: toml::Value = toml::from_str(&fs::read_to_string(lock).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let expected = "7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0";
    let expected_source =
        format!("git+https://codeberg.org/caniko/rs-veritasium.git?rev={expected}#{expected}");
    let sources = value["package"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|package| {
            package["version"].as_str() == Some("0.19.1")
                && package["name"]
                    .as_str()
                    .is_some_and(|name| name == "bevy" || name.starts_with("bevy_"))
        })
        .map(|package| package.get("source").and_then(toml::Value::as_str))
        .collect::<Vec<_>>();
    if sources.is_empty()
        || sources
            .iter()
            .any(|source| *source != Some(expected_source.as_str()))
    {
        return Err("packaged adopter did not resolve the accepted Veritasium revision".into());
    }
    Ok(())
}

fn prepare_runtime_workspace(source: &Path, destination: &Path) -> Result<(), String> {
    let clone = Command::new("git")
        .args(["clone", "--no-local"])
        .arg(source)
        .arg(destination)
        .output()
        .map_err(|e| e.to_string())?;
    require_success("clone runtime workspace", &clone)?;
    let manifest_path = destination.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
    let without_member = manifest.replace("    \"crates/openpencil_ui_schema\",\n", "");
    let without_path = without_member.replace(
        "openpencil_ui_schema = { path = \"crates/openpencil_ui_schema\", version = \"=0.1.0-rc.1\" }",
        "openpencil_ui_schema = { version = \"=0.1.0-rc.1\" }",
    );
    if without_path == manifest {
        return Err("temporary runtime workspace rewrite matched nothing".into());
    }
    fs::write(manifest_path, without_path).map_err(|e| e.to_string())
}

fn validate_runtime_manifest(text: &str) -> Result<(), String> {
    validate_runtime_manifest_version(text, VERSION)
}

fn validate_runtime_manifest_version(text: &str, version: &str) -> Result<(), String> {
    let manifest: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = manifest.get(section).and_then(toml::Value::as_table) {
            for (name, dependency) in table {
                for key in ["path", "git", "registry", "registry-index", "source"] {
                    if dependency.get(key).is_some() {
                        return Err(format!(
                            "packaged runtime retains {key} dependency source for {name}"
                        ));
                    }
                }
            }
        }
    }
    let schema = manifest
        .get("dependencies")
        .and_then(|value| value.get("openpencil_ui_schema"))
        .ok_or("packaged runtime has no schema dependency")?;
    let found = match schema {
        toml::Value::String(value) => value.as_str(),
        toml::Value::Table(table) => table
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or("packaged runtime schema dependency has no version")?,
        _ => return Err("packaged runtime schema dependency has no version".into()),
    };
    if found != format!("={version}") && found != version {
        return Err(format!("packaged runtime schema version is {found}"));
    }
    Ok(())
}

fn unique_temp_root() -> Result<PathBuf, String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "opui-package-preflight-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn crate_file(target: &Path, name: &str) -> Result<PathBuf, String> {
    let package = target.join("package");
    fs::read_dir(&package)
        .map_err(|e| format!("{}: {e}", package.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "crate")
                && path
                    .file_name()
                    .is_some_and(|file| file.to_string_lossy().starts_with(name))
        })
        .ok_or_else(|| format!("no {name} .crate under {}", package.display()))
}

fn unpack(package: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination).map_err(|e| e.to_string())?;
    let output = Command::new("tar")
        .args(["-xzf"])
        .arg(package)
        .args(["-C"])
        .arg(destination)
        .output()
        .map_err(|e| e.to_string())?;
    require_success("unpack package", &output)
}

fn cargo<const N: usize>(root: &Path, args: [&str; N]) -> Result<Output, String> {
    cargo_vec(root, args.into_iter().map(str::to_string).collect())
}

fn cargo_vec(root: &Path, args: Vec<String>) -> Result<Output, String> {
    Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())
}

pub(crate) fn rc2_stable_rust_version() -> Result<String, String> {
    let cargo = Command::new("cargo")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    require_success("cargo --version", &cargo)?;
    let cargo = String::from_utf8_lossy(&cargo.stdout);
    if !cargo.starts_with("cargo 1.95.0 ") {
        return Err(format!(
            "RC2 packaging requires stable cargo 1.95.0, found {}",
            cargo.trim()
        ));
    }
    let rustc = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|e| e.to_string())?;
    require_success("rustc -vV", &rustc)?;
    let text = String::from_utf8_lossy(&rustc.stdout);
    let release = text
        .lines()
        .find_map(|line| line.strip_prefix("release: "))
        .ok_or("rustc -vV has no release")?;
    if release != "1.95.0" {
        return Err(format!(
            "RC2 packaging requires stable rustc 1.95.0, found {release}"
        ));
    }
    Ok(release.into())
}

fn require_success(label: &str, output: &Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{label} failed\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_runtime_rejects_path_dependencies() {
        let manifest = r#"
            [dependencies.openpencil_ui_schema]
            version = "=0.1.0-rc.1"
            path = "../schema"
        "#;
        assert!(
            validate_runtime_manifest(manifest)
                .unwrap_err()
                .contains("path dependency source")
        );
    }

    #[test]
    fn packaged_runtime_rejects_non_registry_sources() {
        let manifest = r#"
            [dependencies.openpencil_ui_schema]
            version = "=0.1.0-rc.1"
            git = "https://example.invalid/schema"
        "#;
        assert!(
            validate_runtime_manifest(manifest)
                .unwrap_err()
                .contains("git dependency source")
        );
    }

    #[test]
    fn packaged_runtime_accepts_exact_schema_version() {
        let manifest = r#"
            [dependencies.openpencil_ui_schema]
            version = "=0.1.0-rc.1"
        "#;
        validate_runtime_manifest(manifest).unwrap();
    }

    #[test]
    fn rc2_packaged_runtime_accepts_exact_schema_version() {
        let manifest = r#"
            [dependencies.openpencil_ui_schema]
            version = "=0.1.0-rc.2"
        "#;
        validate_runtime_manifest_version(manifest, RC2_VERSION).unwrap();
    }

    #[test]
    fn adopter_lock_rejects_any_missing_bevy_source() {
        let sha = "7cd58438d458d3e701c32d5d5ae0c7b1f70a2bc0";
        let lock = std::env::temp_dir().join(format!("opui-adopter-lock-{}", std::process::id()));
        fs::write(
            &lock,
            format!(
                "version = 4\n\n[[package]]\nname = \"bevy\"\nversion = \"0.19.1\"\nsource = \"git+https://codeberg.org/caniko/rs-veritasium.git?rev={sha}#{sha}\"\n\n[[package]]\nname = \"bevy_app\"\nversion = \"0.19.1\"\n"
            ),
        )
        .unwrap();
        assert!(validate_consumer_bevy_pin(&lock).is_err());
        let _ = fs::remove_file(lock);
    }
}
