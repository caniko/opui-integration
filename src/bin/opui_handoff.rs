use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or(
        "usage: opui-handoff prepare|rehearse|rehearse-rc2|import-external|finalize|assess|materialize-public|close-public-sources|generate-rc2-external|bundle-index|verify-bundle-index ...",
    )?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match command.as_str() {
        "prepare" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let manifest = opui_integration::handoff::prepare(&root, &capsule, &output)?;
            println!("{}", manifest.display());
            Ok(())
        }
        "rehearse" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let report = opui_integration::rehearsal::rehearse(&root, &capsule, &output)?;
            println!("{}", report.display());
            Ok(())
        }
        "rehearse-rc2" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let report = opui_integration::rehearsal::rehearse_rc2(&root, &capsule, &output)?;
            println!("{}", report.display());
            Ok(())
        }
        "import-external" => {
            let profile = args.next().ok_or("missing PROFILE")?;
            let source = PathBuf::from(args.next().ok_or("missing RESULT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let destination = opui_integration::external_results::import(&root, &profile, &source)?;
            println!("{}", destination.display());
            Ok(())
        }
        "finalize" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let report = opui_integration::handoff::finalize(&root, &capsule, &output)?;
            println!("{}", report.display());
            Ok(())
        }
        "assess" => {
            let profile = args.next().ok_or("missing PROFILE")?;
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let report = opui_integration::release_profile::assess_capsule(
                &root, &profile, &capsule, &output,
            )?;
            println!("{}", report.display());
            Ok(())
        }
        "materialize-public" => {
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            opui_integration::public_source::materialize(&root)?;
            Ok(())
        }
        "close-public-sources" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            let evidence: Vec<PathBuf> = args.map(PathBuf::from).collect();
            let path =
                opui_integration::public_source::generate(&root, &capsule, &evidence, &output)?;
            println!("{}", path.display());
            Ok(())
        }
        "generate-rc2-external" => {
            let capsule = PathBuf::from(args.next().ok_or("missing CAPSULE")?);
            let closure = PathBuf::from(args.next().ok_or("missing CLOSURE")?);
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            let path = opui_integration::external_results::generate_rc2(
                &root, &capsule, &closure, &output,
            )?;
            println!("{}", path.display());
            Ok(())
        }
        "bundle-index" => {
            let output = PathBuf::from(args.next().ok_or("missing OUTPUT")?);
            let directories: Vec<PathBuf> = args.map(PathBuf::from).collect();
            if directories.is_empty() {
                return Err("missing evidence directory".into());
            }
            let path =
                opui_integration::public_source::write_bundle_index(&root, &directories, &output)?;
            println!("{}", path.display());
            Ok(())
        }
        "verify-bundle-index" => {
            let index = PathBuf::from(args.next().ok_or("missing INDEX")?);
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            opui_integration::public_source::validate_bundle_index(&root, &index)
        }
        _ => Err(format!("unknown command {command}")),
    }
}
