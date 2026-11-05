//! bookcraft-app: L3 desktop shelf for BookCraft (eframe/egui) plus `--smoke` headless checks.
//!
//! The GUI calls `bookcraft_core::dispatch` in-process. `--smoke` runs the same parity
//! checks without opening a window for CI.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

mod app;

use app::{run_smoke_checks, BookApp};

fn print_usage() {
    eprintln!("usage: bookcraft-app [--smoke]");
    eprintln!("  --smoke   headless parity checks (no window), exit 0 on success");
    eprintln!("  no args   open the BookCraft desktop shelf");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--smoke") {
        match run_smoke_checks() {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
                println!("smoke: PASS");
            }
            Err(error) => {
                eprintln!("[bookcraft-app] smoke FAILED: {error}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_usage();
        return;
    }

    let run = eframe::run_native(
        "BookCraft",
        eframe::NativeOptions::default(),
        Box::new(
            |cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::new(BookApp::new(cc)))
            },
        ),
    );
    if let Err(error) = run {
        eprintln!("[bookcraft-app] failed to start: {error}");
        std::process::exit(1);
    }
}
