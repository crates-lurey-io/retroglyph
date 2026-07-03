use std::process;

fn main() {
    let res = cargo_run_bin::cli::run();
    if let Err(res) = res {
        eprintln!("\x1b[31mrun-bin failed: {res}\x1b[0m");
        process::exit(1);
    }
}
