//! Benchmarks `codebroker init` end-to-end against the in-tree fixture
//! repos (or any directories passed on the command line), reporting a cold
//! (full rebuild) and warm (no-op incremental) run for each. `init` itself
//! reports detailed per-stage timing and throughput via `[TIMING]` lines on
//! stderr; this harness just drives repeated runs and reports the headline
//! wall-clock numbers Phase 17 targets: cold <8s for a few hundred files,
//! warm (no changes) <200ms.
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn codebroker_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("failed to resolve current exe");
    path.pop();
    path.push(if cfg!(windows) {
        "codebroker.exe"
    } else {
        "codebroker"
    });
    path
}

fn run_init(bin: &PathBuf, dir: &std::path::Path) -> Duration {
    let start = Instant::now();
    let status = Command::new(bin)
        .arg("init")
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("failed to run codebroker init");
    assert!(status.success(), "codebroker init failed in {}", dir.display());
    start.elapsed()
}

fn main() {
    let bin = codebroker_bin();
    if !bin.exists() {
        eprintln!(
            "codebroker binary not found at {} — build it first (cargo build -p cli)",
            bin.display()
        );
        std::process::exit(1);
    }

    let cwd = std::env::current_dir().expect("failed to resolve cwd");
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let targets: Vec<PathBuf> = if args.is_empty() {
        ["testing/nextjs_ecommerce", "testing/python_taskflow", "testing/javascript_crm"]
            .iter()
            .map(|p| cwd.join(p))
            .collect()
    } else {
        args
    };

    println!("Benchmarking `codebroker init` (binary: {})", bin.display());
    for target in &targets {
        if !target.is_dir() {
            println!("  {:<45} SKIPPED (not found)", target.display());
            continue;
        }
        let _ = std::fs::remove_dir_all(target.join(".codebroker"));
        let cold = run_init(&bin, target);
        let warm = run_init(&bin, target);
        println!(
            "  {:<45} cold={:>9.2?}  warm(no-op)={:>9.2?}",
            target.display(),
            cold,
            warm
        );
        let _ = std::fs::remove_dir_all(target.join(".codebroker"));
    }
}
