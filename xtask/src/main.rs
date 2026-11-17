//! Repository automation. Run with `cargo xtask <command>`.

fn main() {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        None | Some("help") => println!("usage: cargo xtask <command>\ncommands: (none yet)"),
        Some(other) => {
            eprintln!("unknown xtask command: {other}");
            std::process::exit(1);
        }
    }
}
