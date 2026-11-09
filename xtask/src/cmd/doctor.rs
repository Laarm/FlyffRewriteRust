//! `cargo xtask doctor`: tool versions, Rust targets/components, Node/pnpm, `FLYFF_RESOURCE` /
//! `FLYFF_SOURCE` plausibility, free disk space (Plan/02 §4.2).

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::build_id;
use crate::proc::{Ctx, Run};
use crate::tools;
use anyhow::{Result, bail};
use std::path::Path;

/// State of an environment variable that points at a data tree.
#[derive(Debug, PartialEq, Eq)]
pub enum TreeState {
    /// Variable not set.
    Unset,
    /// Set, directory exists and contains the marker.
    Valid,
    /// Set but the path is missing or lacks the marker.
    Invalid(String),
}

/// Checks `value` (an env var's value) for a case-insensitive marker entry inside the directory.
pub fn classify_tree(value: Option<&std::ffi::OsStr>, marker: &str) -> TreeState {
    let Some(v) = value.filter(|v| !v.is_empty()) else {
        return TreeState::Unset;
    };
    let dir = Path::new(v);
    if !dir.is_dir() {
        return TreeState::Invalid(format!("{} is not a directory", dir.display()));
    }
    let has = std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(marker))
    });
    if has {
        TreeState::Valid
    } else {
        TreeState::Invalid(format!("{} has no `{marker}` entry", dir.display()))
    }
}

fn report_tree(label: &str, value: Option<std::ffi::OsString>, marker: &str, advice: &str) {
    match classify_tree(value.as_deref(), marker) {
        TreeState::Unset => println!("{label:<16} not set ({advice})"),
        TreeState::Valid => println!(
            "{label:<16} set and valid: {}",
            value.unwrap_or_default().to_string_lossy()
        ),
        TreeState::Invalid(why) => println!("{label:<16} SET BUT INVALID: {why}"),
    }
}

/// Free space in KiB on the filesystem holding `path` (via `df -Pk`).
pub fn free_kib(ctx: &Ctx, path: &Path) -> Option<u64> {
    let out = Run::new(ctx, "df").arg("-Pk").arg(path).output().ok()?;
    out.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

/// Runs the doctor.
pub fn run(ctx: &Ctx) -> Result<()> {
    println!("== flyff doctor ==");
    println!("repository:      {}", ctx.root.display());
    println!(
        "build id:        {}",
        build_id::compute(ctx).unwrap_or_else(|e| format!("unknown ({e})"))
    );
    println!(
        "host:            {} cores, {}",
        std::thread::available_parallelism().map_or(1, usize::from),
        std::env::consts::OS
    );
    println!();
    println!("-- tools --");
    let mut ok = tools::print_table(&tools::check_all(ctx));

    println!();
    println!("-- rust targets and components --");
    let checks: [(&str, [&str; 3], &[&str]); 2] = [
        (
            "target",
            ["target", "list", "--installed"],
            &["wasm32-unknown-unknown", "x86_64-unknown-linux-musl"],
        ),
        (
            "component",
            ["component", "list", "--installed"],
            &["rustfmt", "clippy", "llvm-tools"],
        ),
    ];
    for (what, args, wanted) in checks {
        match Run::new(ctx, "rustup").args(args).output() {
            Ok(list) => {
                for w in wanted {
                    let present = list.lines().any(|l| l.starts_with(w));
                    println!(
                        "{what:<10} {w:<28} {}",
                        if present {
                            "ok"
                        } else {
                            "MISSING (rustup show installs the pin)"
                        }
                    );
                    ok &= present;
                }
            }
            Err(e) => println!("{what:<10} cannot query rustup: {e:#}"),
        }
    }

    println!();
    println!("-- environment --");
    report_tree(
        "FLYFF_RESOURCE",
        std::env::var_os("FLYFF_RESOURCE"),
        "Masquerade.prj",
        "res_ tests skip, `flyff` needs --resource",
    );
    report_tree(
        "FLYFF_SOURCE",
        std::env::var_os("FLYFF_SOURCE"),
        "_Common",
        "oracle and npcscript-transpile need it",
    );
    println!(
        "FLYFF_HOME       {}",
        std::env::var("FLYFF_HOME")
            .unwrap_or_else(|_| "not set (home = folder of the executable)".to_owned())
    );
    match free_kib(ctx, &ctx.root) {
        Some(kib) => {
            let gib = kib as f64 / (1024.0 * 1024.0);
            println!(
                "free disk space  {gib:.1} GiB{}",
                if gib < 5.0 {
                    "  (LOW: a full build needs several GiB)"
                } else {
                    ""
                }
            );
        }
        None => println!("free disk space  unknown"),
    }
    println!();
    if ok {
        println!("doctor: OK");
        Ok(())
    } else {
        bail!("doctor found problems in required tools (see above)")
    }
}
