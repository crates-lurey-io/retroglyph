use std::process;

fn main() {
    let res = cargo_run_bin::cli::run();
    if let Err(res) = res {
        eprintln!("\x1b[31m{}\x1b[0m", format!("run-bin failed: {res}"));
        process::exit(1);
    }
}
