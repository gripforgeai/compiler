//! The command `gripforge-build` (see [`gripforge_compiler::cli`]).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code =
        gripforge_compiler::cli::main(&args, &mut |s| println!("{s}"), &mut |s| eprintln!("{s}"));
    std::process::exit(code);
}
