use std::fs;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::handoff::{RUNTIME_SHA256, SCHEMA_SHA256, VERITASIUM_SHA, VERSION};

const CRATES_IO_INDEX: &str = "https://github.com/rust-lang/crates.io-index";
const RC2_VERSION: &str = "0.1.0-rc.2";

#[derive(Serialize)]
struct RehearsalReport {
    schema_version: u32,
    status: &'static str,
    publication_performed: bool,
    registry: &'static str,
    package_order: [&'static str; 2],
    packages: [PackageReport; 2],
    consumers: [ConsumerReport; 2],
    expected_operator_commands: Vec<String>,
    propagation_check: String,
    abort_conditions: Vec<&'static str>,
}

#[derive(Serialize)]
struct PackageReport {
    name: &'static str,
    version: String,
    sha256: String,
    repository: &'static str,
    license: &'static str,
    readme: &'static str,
    rust_version: String,
    features: Vec<&'static str>,
}

#[derive(Serialize)]
struct ConsumerReport {
    name: &'static str,
    package: &'static str,
    checksum: String,
    owner_paths_absent: bool,
    burn_in_cycles: u32,
    status: &'static str,
}

struct Server(Child);

struct LoopbackRegistry {
    url: String,
    root: PathBuf,
    _server: Server,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn rehearse(root: &Path, capsule: &Path, output: &Path) -> Result<PathBuf, String> {
    rehearse_version(
        root,
        capsule,
        output,
        VERSION,
        SCHEMA_SHA256,
        RUNTIME_SHA256,
    )
}

pub fn rehearse_rc2(root: &Path, capsule: &Path, output: &Path) -> Result<PathBuf, String> {
    let capsule = resolve(root, capsule);
    let version = RC2_VERSION;
    let schema_sha = crate::cert::sha256_file(
        &capsule.join(format!("artifacts/openpencil_ui_schema-{version}.crate")),
    )?;
    let runtime_sha = crate::cert::sha256_file(
        &capsule.join(format!("artifacts/bevy_openpencil-{version}.crate")),
    )?;
    rehearse_version(root, &capsule, output, version, &schema_sha, &runtime_sha)
}

fn rehearse_version(
    root: &Path,
    capsule: &Path,
    output: &Path,
    version: &str,
    schema_sha: &str,
    runtime_sha: &str,
) -> Result<PathBuf, String> {
    let (registry_name, rust_version) = match version {
        VERSION => ("opui-rc1", "1.95".into()),
        RC2_VERSION => (
            "opui-rc2",
            crate::package_preflight::rc2_stable_rust_version()?,
        ),
        _ => return Err(format!("unsupported rehearsal version {version}")),
    };
    let capsule = resolve(root, capsule);
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    if output.exists() {
        return Err(format!(
            "rehearsal output already exists: {}",
            output.display()
        ));
    }
    fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let temp = output.join("work");
    let schema_archive = capsule.join(format!("artifacts/openpencil_ui_schema-{version}.crate"));
    let loopback = serve_schema_archive(&temp, &schema_archive, version, schema_sha)?;
    let registry = loopback.root.clone();
    let index = registry.join("index");
    let registry_url = loopback.url.clone();
    let cargo_home = temp.join("cargo-home");
    fs::create_dir_all(&cargo_home).map_err(|e| e.to_string())?;

    let schema_consumer = temp.join("schema-consumer");
    write_consumer(
        &schema_consumer,
        "schema-rehearsal",
        &format!(
            "openpencil_ui_schema = {{ version = \"={version}\", registry = \"{registry_name}\" }}"
        ),
        "use openpencil_ui_schema as _; fn main() {}\n",
        &registry_url,
        registry_name,
    )?;
    cargo_check(&schema_consumer, &cargo_home)?;
    validate_lock(
        &schema_consumer.join("Cargo.lock"),
        "openpencil_ui_schema",
        version,
        schema_sha,
        root,
    )?;

    stage_archive(&capsule, &registry, "bevy_openpencil", version, runtime_sha)?;
    write_index(
        &index,
        "bevy_openpencil",
        &runtime_index(&registry_url, version, runtime_sha),
    )?;
    let runtime_consumer = temp.join("runtime-consumer");
    write_consumer(
        &runtime_consumer,
        "runtime-rehearsal",
        &format!(
            "bevy = {{ version = \"=0.19.1\", default-features = false, features = [\"std\", \"bevy_asset\", \"bevy_ui\"] }}\nbevy_openpencil = {{ version = \"={version}\", registry = \"{registry_name}\", default-features = false }}\n\n[patch.crates-io]\nbevy = {{ git = \"https://codeberg.org/caniko/rs-veritasium.git\", rev = \"{VERITASIUM_SHA}\" }}\nopenpencil_ui_schema = {{ version = \"={version}\", registry = \"{registry_name}\" }}"
        ),
        "use bevy::prelude::*; use bevy_openpencil::OpenPencilUiPlugin; fn main() { let mut app = App::new(); app.add_plugins((MinimalPlugins, AssetPlugin::default(), OpenPencilUiPlugin)); for _ in 0..100 { app.update(); } println!(\"burn-in-cycles=100\"); }\n",
        &registry_url,
        registry_name,
    )?;
    cargo_check(&runtime_consumer, &cargo_home)?;
    validate_lock(
        &runtime_consumer.join("Cargo.lock"),
        "openpencil_ui_schema",
        version,
        schema_sha,
        root,
    )?;
    validate_lock(
        &runtime_consumer.join("Cargo.lock"),
        "bevy_openpencil",
        version,
        runtime_sha,
        root,
    )?;
    let lock =
        fs::read_to_string(runtime_consumer.join("Cargo.lock")).map_err(|e| e.to_string())?;
    let expected_source = format!(
        "git+https://codeberg.org/caniko/rs-veritasium.git?rev={VERITASIUM_SHA}#{VERITASIUM_SHA}"
    );
    if !lock_has_exact_bevy_source(&lock, &expected_source)? {
        return Err("runtime rehearsal did not resolve canonical Veritasium".into());
    }
    let burn_in = cargo_run(&runtime_consumer, &cargo_home)?;
    if !burn_in.contains("burn-in-cycles=100") {
        return Err("runtime adopter did not complete its bounded burn-in".into());
    }

    let report = RehearsalReport {
        schema_version: 2,
        status: "pass",
        publication_performed: false,
        registry: "isolated-loopback-sparse-registry",
        package_order: ["openpencil_ui_schema", "bevy_openpencil"],
        packages: [
            PackageReport {
                name: "openpencil_ui_schema",
                version: version.into(),
                sha256: schema_sha.into(),
                repository: "https://github.com/caniko/bevy_openpencil",
                license: "MIT",
                readme: "README.md",
                rust_version: rust_version.clone(),
                features: vec![],
            },
            PackageReport {
                name: "bevy_openpencil",
                version: version.into(),
                sha256: runtime_sha.into(),
                repository: "https://github.com/caniko/bevy_openpencil",
                license: "MIT",
                readme: "README.md",
                rust_version,
                features: vec!["default", "default_font", "file_watcher"],
            },
        ],
        consumers: [
            ConsumerReport {
                name: "schema-rehearsal",
                package: "openpencil_ui_schema",
                checksum: schema_sha.into(),
                owner_paths_absent: true,
                burn_in_cycles: 0,
                status: "pass",
            },
            ConsumerReport {
                name: "runtime-rehearsal",
                package: "bevy_openpencil + exact schema",
                checksum: runtime_sha.into(),
                owner_paths_absent: true,
                burn_in_cycles: 100,
                status: "pass",
            },
        ],
        expected_operator_commands: vec![
            "cargo publish --locked -p openpencil_ui_schema".into(),
            format!("cargo info openpencil_ui_schema@{version}"),
            "cargo publish --locked -p bevy_openpencil".into(),
            format!("cargo info bevy_openpencil@{version}"),
        ],
        propagation_check: format!(
            "cargo info <crate>@{version} and fresh locked downstream cargo check"
        ),
        abort_conditions: vec![
            "archive checksum mismatch",
            "existing crate/version",
            "schema unavailable before runtime",
            "owner path or path patch required",
            "packaged adopter burn-in does not complete",
            "metadata, MSRV, license, readme, feature, API, or semver mismatch",
            "missing publication authority",
        ],
    };
    let report_path = output.join("publication-rehearsal.json");
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::remove_dir_all(temp).map_err(|e| e.to_string())?;
    Ok(report_path)
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn lock_has_exact_bevy_source(text: &str, expected: &str) -> Result<bool, String> {
    let lock: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let sources = lock["package"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|package| {
            package["version"].as_str() == Some("0.19.1")
                && package["name"]
                    .as_str()
                    .is_some_and(|name| name == "bevy" || name.starts_with("bevy_"))
        })
        .map(|package| package["source"].as_str())
        .collect::<Vec<_>>();
    Ok(!sources.is_empty() && sources.iter().all(|source| *source == Some(expected)))
}

fn cargo_run(root: &Path, cargo_home: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["run", "--locked", "--quiet"])
        .env("CARGO_HOME", cargo_home)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "packaged adopter burn-in failed\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn serve_schema_archive(
    work: &Path,
    archive: &Path,
    version: &str,
    checksum: &str,
) -> Result<LoopbackRegistry, String> {
    let actual = crate::cert::sha256_file(archive)?;
    if actual != checksum {
        return Err(format!(
            "openpencil_ui_schema archive checksum mismatch: {actual}"
        ));
    }
    let root = work.join("registry");
    let index = root.join("index");
    fs::create_dir_all(&index).map_err(|e| e.to_string())?;
    let port = unused_port()?;
    write_sparse_config(&index, port)?;
    stage_download(&root, "openpencil_ui_schema", version, archive)?;
    write_index(
        &index,
        "openpencil_ui_schema",
        &schema_index_metadata(version, checksum),
    )?;
    let server = start_server(&root, port)?;
    Ok(LoopbackRegistry {
        url: format!("sparse+http://127.0.0.1:{port}/index/"),
        root,
        _server: server,
    })
}

fn write_sparse_config(index: &Path, port: u16) -> Result<(), String> {
    fs::write(
        index.join("config.json"),
        format!(
            "{{\"dl\":\"http://127.0.0.1:{port}/crates/{{crate}}/{{version}}/download\",\"api\":null}}"
        ),
    )
    .map_err(|e| e.to_string())
}

fn unused_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| e.to_string())?
        .local_addr()
        .map(|address| address.port())
        .map_err(|e| e.to_string())
}

fn start_server(root: &Path, port: u16) -> Result<Server, String> {
    let child = Command::new("python3")
        .args([
            "-m",
            "http.server",
            &port.to_string(),
            "--bind",
            "127.0.0.1",
            "--directory",
        ])
        .arg(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("local registry server did not start".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(Server(child))
}

fn stage_archive(
    capsule: &Path,
    registry: &Path,
    name: &str,
    version: &str,
    expected: &str,
) -> Result<(), String> {
    let source = capsule.join(format!("artifacts/{name}-{version}.crate"));
    let actual = crate::cert::sha256_file(&source)?;
    if actual != expected {
        return Err(format!("{name} archive checksum mismatch: {actual}"));
    }
    stage_download(registry, name, version, &source)
}

fn stage_download(registry: &Path, name: &str, version: &str, source: &Path) -> Result<(), String> {
    let destination = registry.join(format!("crates/{name}/{version}/download"));
    fs::create_dir_all(destination.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn write_index(index: &Path, name: &str, value: &serde_json::Value) -> Result<(), String> {
    let path = index.join(index_path(name));
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(path, format!("{}\n", serde_json::to_string(value).unwrap()))
        .map_err(|e| e.to_string())
}

fn index_path(name: &str) -> PathBuf {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        1 => PathBuf::from("1").join(lower),
        2 => PathBuf::from("2").join(lower),
        3 => PathBuf::from("3").join(&lower[..1]).join(lower),
        _ => PathBuf::from(&lower[..2]).join(&lower[2..4]).join(lower),
    }
}

fn dep(
    name: &str,
    req: &str,
    features: &[&str],
    default_features: bool,
    registry: &str,
) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "req": req,
        "features": features,
        "optional": false,
        "default_features": default_features,
        "target": null,
        "kind": "normal",
        "registry": registry,
        "package": null,
    })
}

fn schema_index_metadata(version: &str, cksum: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "openpencil_ui_schema",
        "vers": version,
        "deps": [
            dep("serde", "^1", &["derive"], true, CRATES_IO_INDEX),
            dep("serde_json", "^1", &[], true, CRATES_IO_INDEX),
            dep("thiserror", "^2", &[], true, CRATES_IO_INDEX),
        ],
        "cksum": cksum,
        "features": {},
        "yanked": false,
        "links": null,
        "rust_version": "1.95",
        "v": 2,
    })
}

fn runtime_index(registry_url: &str, version: &str, checksum: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "bevy_openpencil",
        "vers": version,
        "deps": [
            dep("bevy", "=0.19.1", &["std", "bevy_asset", "bevy_color", "bevy_image", "bevy_input_focus", "bevy_log", "bevy_picking", "bevy_sprite", "bevy_text", "bevy_ui", "bevy_ui_widgets", "bevy_window", "png"], false, CRATES_IO_INDEX),
            dep("openpencil_ui_schema", &format!("={version}"), &[], true, registry_url),
            dep("serde", "^1", &["derive"], true, CRATES_IO_INDEX),
            dep("serde_json", "^1", &[], true, CRATES_IO_INDEX),
            dep("sha2", "^0.10", &[], true, CRATES_IO_INDEX),
            dep("thiserror", "^2", &[], true, CRATES_IO_INDEX),
        ],
        "cksum": checksum,
        "features": {
            "default": ["default_font"],
            "default_font": ["bevy/default_font"],
            "file_watcher": ["bevy/file_watcher"],
        },
        "yanked": false,
        "links": null,
        "rust_version": "1.95",
        "v": 2,
    })
}

fn write_consumer(
    root: &Path,
    name: &str,
    dependencies: &str,
    source: &str,
    registry_url: &str,
    registry_name: &str,
) -> Result<(), String> {
    fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;
    fs::create_dir_all(root.join(".cargo")).map_err(|e| e.to_string())?;
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}\n"
        ),
    )
    .map_err(|e| e.to_string())?;
    fs::write(root.join("src/main.rs"), source).map_err(|e| e.to_string())?;
    fs::write(
        root.join(".cargo/config.toml"),
        format!("[registries.{registry_name}]\nindex = \"{registry_url}\"\n"),
    )
    .map_err(|e| e.to_string())
}

fn cargo_check(root: &Path, cargo_home: &Path) -> Result<(), String> {
    let lock = Command::new("cargo")
        .arg("generate-lockfile")
        .env("CARGO_HOME", cargo_home)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !lock.status.success() {
        return Err(format!(
            "cargo generate-lockfile failed\n{}{}",
            String::from_utf8_lossy(&lock.stdout),
            String::from_utf8_lossy(&lock.stderr)
        ));
    }
    let output = Command::new("cargo")
        .args(["check", "--locked"])
        .env("CARGO_HOME", cargo_home)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo check failed\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn validate_lock(
    path: &Path,
    package: &str,
    version: &str,
    checksum: &str,
    owner_root: &Path,
) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if text.contains(owner_root.to_string_lossy().as_ref()) || text.contains("path+") {
        return Err(format!(
            "{} contains an owner path dependency",
            path.display()
        ));
    }
    let lock: toml::Value = toml::from_str(&text).map_err(|e| e.to_string())?;
    let found = lock["package"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|entry| {
            entry["name"].as_str() == Some(package)
                && entry["version"].as_str() == Some(version)
                && entry["checksum"].as_str() == Some(checksum)
        });
    if found {
        Ok(())
    } else {
        Err(format!(
            "{package} checksum missing from {}",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_paths_match_cargo_layout() {
        assert_eq!(index_path("a"), PathBuf::from("1/a"));
        assert_eq!(index_path("ab"), PathBuf::from("2/ab"));
        assert_eq!(index_path("abc"), PathBuf::from("3/a/abc"));
        assert_eq!(
            index_path("bevy_openpencil"),
            PathBuf::from("be/vy/bevy_openpencil")
        );
    }

    #[test]
    fn staged_metadata_uses_exact_schema_dependency() {
        let runtime = runtime_index("sparse+http://127.0.0.1/index/", VERSION, RUNTIME_SHA256);
        assert!(
            runtime["deps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|dependency| {
                    dependency["name"] == "openpencil_ui_schema"
                        && dependency["req"] == "=0.1.0-rc.1"
                })
        );
    }

    #[test]
    fn rc2_registry_index_requires_version_and_checksum() {
        let index = schema_index_metadata("0.1.0-rc.2", "deadbeef");
        assert_eq!(index["vers"], "0.1.0-rc.2");
        assert_eq!(index["cksum"], "deadbeef");
        assert_eq!(index["name"], "openpencil_ui_schema");
    }

    #[test]
    fn rc2_runtime_metadata_uses_exact_schema_dependency() {
        let runtime = runtime_index("sparse+http://127.0.0.1/index/", "0.1.0-rc.2", "hash");
        assert!(
            runtime["deps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|dependency| {
                    dependency["name"] == "openpencil_ui_schema"
                        && dependency["req"] == "=0.1.0-rc.2"
                })
        );
    }

    #[test]
    fn bevy_lock_source_must_match_exactly() {
        let expected = "git+https://codeberg.org/caniko/rs-veritasium.git?rev=abc#abc";
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"bevy\"\nversion = \"0.19.1\"\nsource = \"{expected}\"\n"
        );
        assert!(lock_has_exact_bevy_source(&lock, expected).unwrap());
        assert!(!lock_has_exact_bevy_source(&lock, "abc").unwrap());
    }
}
