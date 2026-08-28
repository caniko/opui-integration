use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (check, input, output) = match args.as_slice() {
        [input, output] => (false, input, output),
        [flag, input, output] if flag == "--check" => (true, input, output),
        _ => usage(),
    };
    if let Err(error) =
        opui_integration::bindings::write_or_check(Path::new(&input), Path::new(&output), check)
    {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("usage: opui-bindings [--check] INPUT.opui OUTPUT.rs");
    std::process::exit(2);
}
