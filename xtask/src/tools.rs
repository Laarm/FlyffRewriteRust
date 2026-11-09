//! Tool pins and checks (Plan/02 §3).
//!
//! `xtask` prepends `<repo>/.tools/bin` to `PATH` for every child process; nothing is installed
//! globally. `tools check` prints a table (tool, wanted, found, status) and exits 1 on any required
//! mismatch with the exact fix command. Divergence from the plan: zig/cargo-zigbuild are replaced by
//! the system `musl-gcc` (package `musl-tools`) for the static build (ADR-017); the downloadable-binary
//! installer with SHA-256 pins is not needed on a host that already carries buf/binaryen/node.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::build_id;
use crate::proc::{Ctx, Run};
use anyhow::{Result, bail};

/// How a tool can be installed by `tools install`.
#[derive(Clone, Copy, Debug)]
pub enum Install {
    /// `cargo install --locked <krate> [--version =<lock>] --root .tools`; the version comes from
    /// `Cargo.lock` when `lock_dep` is set.
    Cargo {
        /// Crate name.
        krate: &'static str,
        /// Package whose `Cargo.lock` version pins the install (wasm-bindgen-cli).
        lock_dep: Option<&'static str>,
    },
    /// Debian/Ubuntu package (installed with `--apt`).
    Apt(&'static str),
    /// Manual instruction only.
    Hint(&'static str),
}

/// What version a tool must have.
#[derive(Clone, Copy, Debug)]
pub enum Want {
    /// Any working installation.
    Present,
    /// At least this dotted version.
    AtLeast(&'static str),
    /// This major version (`node` LTS).
    Major(u64),
    /// Exactly the version of this package in `Cargo.lock` (wasm-bindgen-cli).
    LockedTo(&'static str),
    /// Exactly the `channel` of `rust-toolchain.toml`.
    ToolchainChannel,
}

/// One pinned tool.
#[derive(Clone, Copy, Debug)]
pub struct ToolPin {
    /// Display name.
    pub name: &'static str,
    /// Probe command (program + args printing the version).
    pub probe: &'static [&'static str],
    /// Version requirement.
    pub want: Want,
    /// A missing/mismatching required tool fails `tools check`.
    pub required: bool,
    /// Which commands need it.
    pub needed_for: &'static str,
    /// Installation method.
    pub install: Install,
}

/// The pin table (Plan/02 §3.1).
pub const TOOLS: &[ToolPin] = &[
    ToolPin {
        name: "rustc",
        probe: &["rustc", "--version"],
        want: Want::ToolchainChannel,
        required: true,
        needed_for: "everything",
        install: Install::Hint("rustup show (installs the pin of rust-toolchain.toml)"),
    },
    ToolPin {
        name: "cargo",
        probe: &["cargo", "--version"],
        want: Want::Present,
        required: true,
        needed_for: "everything",
        install: Install::Hint("install rustup: https://rustup.rs"),
    },
    ToolPin {
        name: "rustfmt",
        probe: &["rustfmt", "--version"],
        want: Want::Present,
        required: true,
        needed_for: "check, fmt",
        install: Install::Hint("rustup component add rustfmt"),
    },
    ToolPin {
        name: "clippy",
        probe: &["cargo", "clippy", "--version"],
        want: Want::Present,
        required: true,
        needed_for: "check, lint",
        install: Install::Hint("rustup component add clippy"),
    },
    ToolPin {
        name: "git",
        probe: &["git", "--version"],
        want: Want::Present,
        required: true,
        needed_for: "build id, commits",
        install: Install::Apt("git"),
    },
    ToolPin {
        name: "musl-gcc",
        probe: &["musl-gcc", "-dumpversion"],
        want: Want::Present,
        required: true,
        needed_for: "dist (static musl C code: SQLite, Lua, mimalloc)",
        install: Install::Apt("musl-tools"),
    },
    ToolPin {
        name: "node",
        probe: &["node", "--version"],
        want: Want::Major(24),
        required: false,
        needed_for: "build-web, web tests",
        install: Install::Hint("install Node 24 LTS (see .node-version), e.g. with fnm"),
    },
    ToolPin {
        name: "pnpm",
        probe: &["pnpm", "--version"],
        want: Want::AtLeast("10"),
        required: false,
        needed_for: "build-web",
        install: Install::Hint("corepack enable && corepack prepare pnpm@latest --activate"),
    },
    ToolPin {
        name: "buf",
        probe: &["buf", "--version"],
        want: Want::AtLeast("1.50"),
        required: false,
        needed_for: "gen-proto",
        install: Install::Hint(
            "download a release from https://github.com/bufbuild/buf/releases into .tools/bin",
        ),
    },
    ToolPin {
        name: "wasm-bindgen",
        probe: &["wasm-bindgen", "--version"],
        want: Want::LockedTo("wasm-bindgen"),
        required: false,
        needed_for: "build-wasm",
        install: Install::Cargo {
            krate: "wasm-bindgen-cli",
            lock_dep: Some("wasm-bindgen"),
        },
    },
    ToolPin {
        name: "wasm-opt",
        probe: &["wasm-opt", "--version"],
        // Plan/02 §3.1 pins binaryen >= 123; the distribution package on the reference host is 120, which
        // supports every flag of the plan's wasm-opt command line (recorded in ADR-017).
        want: Want::AtLeast("120"),
        required: false,
        needed_for: "build-wasm (size optimisation)",
        install: Install::Apt("binaryen"),
    },
    ToolPin {
        name: "cargo-nextest",
        probe: &["cargo", "nextest", "--version"],
        want: Want::Present,
        required: false,
        needed_for: "faster test runs (optional)",
        install: Install::Cargo {
            krate: "cargo-nextest",
            lock_dep: None,
        },
    },
    ToolPin {
        name: "cargo-deny",
        probe: &["cargo", "deny", "--version"],
        want: Want::Present,
        required: false,
        needed_for: "license/ban checks (optional)",
        install: Install::Cargo {
            krate: "cargo-deny",
            lock_dep: None,
        },
    },
];

/// Result of checking one tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Meets the requirement.
    Ok,
    /// Not installed.
    Missing,
    /// Installed with the wrong version.
    Mismatch,
}

/// One row of the table.
#[derive(Clone, Debug)]
pub struct Row {
    /// Tool name.
    pub name: &'static str,
    /// Requirement, human readable.
    pub wanted: String,
    /// Detected version.
    pub found: Option<String>,
    /// Verdict.
    pub status: Status,
    /// Required for `tools check` to pass?
    pub required: bool,
    /// Fix command / hint.
    pub fix: String,
    /// What needs the tool.
    pub needed_for: &'static str,
}

/// First dotted version number in `text` (`"v24.22.0"` → `24.22.0`).
pub fn parse_version(text: &str) -> Option<String> {
    let re = regex::Regex::new(r"(\d+(?:\.\d+){0,3})").ok()?;
    re.captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

/// Component-wise numeric comparison of dotted versions (`1.74.0 >= 1.50`).
pub fn version_at_least(found: &str, wanted: &str) -> bool {
    let parse = |s: &str| {
        s.split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let (f, w) = (parse(found), parse(wanted));
    for i in 0..f.len().max(w.len()) {
        let (a, b) = (
            f.get(i).copied().unwrap_or(0),
            w.get(i).copied().unwrap_or(0),
        );
        if a != b {
            return a > b;
        }
    }
    true
}

/// Evaluates `want` against the probe output (pure; the I/O-derived inputs are parameters).
pub fn evaluate(
    pin: &ToolPin,
    probe_output: Option<&str>,
    lock_version: Option<&str>,
    channel: Option<&str>,
) -> (String, Option<String>, Status) {
    let wanted = match pin.want {
        Want::Present => "any".to_owned(),
        Want::AtLeast(v) => format!(">= {v}"),
        Want::Major(m) => format!("{m}.x"),
        Want::LockedTo(pkg) => format!("= {} (Cargo.lock: {pkg})", lock_version.unwrap_or("?")),
        Want::ToolchainChannel => format!("= {} (rust-toolchain.toml)", channel.unwrap_or("?")),
    };
    let Some(out) = probe_output else {
        return (wanted, None, Status::Missing);
    };
    let found = parse_version(out);
    let ok = match (pin.want, &found) {
        (_, None) => matches!(pin.want, Want::Present),
        (Want::Present, _) => true,
        (Want::AtLeast(v), Some(f)) => version_at_least(f, v),
        (Want::Major(m), Some(f)) => {
            f.split('.').next().and_then(|s| s.parse::<u64>().ok()) == Some(m)
        }
        (Want::LockedTo(_), Some(f)) => lock_version.is_some_and(|l| l == f),
        (Want::ToolchainChannel, Some(f)) => channel.is_some_and(|c| c == f),
    };
    (
        wanted,
        found,
        if ok { Status::Ok } else { Status::Mismatch },
    )
}

fn toolchain_channel(ctx: &Ctx) -> Option<String> {
    let text = std::fs::read_to_string(ctx.path("rust-toolchain.toml")).ok()?;
    let doc: toml::Table = text.parse().ok()?;
    doc.get("toolchain")?
        .get("channel")?
        .as_str()
        .map(str::to_owned)
}

fn fix_text(ctx: &Ctx, pin: &ToolPin, lock: Option<&str>) -> String {
    match pin.install {
        Install::Cargo { krate, lock_dep } => {
            let ver = lock_dep
                .and(lock)
                .map_or_else(String::new, |v| format!(" --version ={v}"));
            let _ = ctx;
            format!(
                "cargo xtask tools install   (cargo install --locked {krate}{ver} --root .tools)"
            )
        }
        Install::Apt(pkg) => {
            format!("sudo apt-get install -y {pkg}   (or: cargo xtask tools install --apt)")
        }
        Install::Hint(h) => h.to_owned(),
    }
}

/// Probes every pinned tool.
pub fn check_all(ctx: &Ctx) -> Vec<Row> {
    let channel = toolchain_channel(ctx);
    TOOLS
        .iter()
        .map(|pin| {
            let lock = match pin.want {
                Want::LockedTo(pkg) => build_id::repo_lock_version(ctx, pkg).ok(),
                _ => None,
            };
            let mut run = Run::new(ctx, pin.probe[0]);
            for a in &pin.probe[1..] {
                run = run.arg(a);
            }
            let out = run
                .capture()
                .ok()
                .and_then(|(ok, stdout, stderr)| ok.then(|| format!("{stdout}{stderr}")));
            let (wanted, found, status) =
                evaluate(pin, out.as_deref(), lock.as_deref(), channel.as_deref());
            Row {
                name: pin.name,
                wanted,
                found,
                status,
                required: pin.required,
                fix: fix_text(ctx, pin, lock.as_deref()),
                needed_for: pin.needed_for,
            }
        })
        .collect()
}

/// Prints the table; returns `false` when a tool has the wrong version or a required tool is missing.
pub fn print_table(rows: &[Row]) -> bool {
    println!(
        "{:<14} {:<34} {:<14} {:<9} needed for",
        "tool", "wanted", "found", "status"
    );
    let mut ok = true;
    for r in rows {
        let status = match (r.status, r.required) {
            (Status::Ok, _) => "ok",
            (Status::Missing, true) => "MISSING",
            (Status::Missing, false) => "missing",
            (Status::Mismatch, true) => "MISMATCH",
            (Status::Mismatch, false) => "mismatch",
        };
        println!(
            "{:<14} {:<34} {:<14} {:<9} {}",
            r.name,
            r.wanted,
            r.found.as_deref().unwrap_or("-"),
            status,
            r.needed_for
        );
        // A wrong version always fails (a stale CLI silently breaks builds); a missing tool only when required.
        if r.status == Status::Mismatch || (r.status == Status::Missing && r.required) {
            ok = false;
        }
    }
    for r in rows.iter().filter(|r| r.status != Status::Ok) {
        println!("fix {}: {}", r.name, r.fix);
    }
    ok
}

/// `tools check`.
pub fn cmd_check(ctx: &Ctx) -> Result<()> {
    let rows = check_all(ctx);
    if print_table(&rows) {
        Ok(())
    } else {
        bail!("required tools are missing or have the wrong version (see the fix lines above)")
    }
}

/// `tools path`.
pub fn cmd_path(ctx: &Ctx) {
    println!("{}", ctx.tools_bin().display());
}

/// `tools install [--apt]`: installs what can be installed automatically into `.tools/`.
pub fn cmd_install(ctx: &Ctx, apt: bool) -> Result<()> {
    std::fs::create_dir_all(ctx.tools_bin())?;
    let rows = check_all(ctx);
    let mut failed = Vec::new();
    for (pin, row) in TOOLS.iter().zip(&rows) {
        if row.status == Status::Ok {
            continue;
        }
        match pin.install {
            Install::Cargo { krate, lock_dep } => {
                let mut run = Run::new(ctx, "cargo")
                    .args(["install", "--locked", krate, "--root"])
                    .arg(ctx.root.join(".tools"));
                if let Some(dep) = lock_dep {
                    let v = build_id::repo_lock_version(ctx, dep)?;
                    run = run.arg("--version").arg(format!("={v}"));
                }
                if let Err(e) = run.run() {
                    failed.push(format!("{}: {e:#}", pin.name));
                }
            }
            Install::Apt(pkg) if apt => {
                if let Err(e) = Run::new(ctx, "sudo")
                    .args(["apt-get", "install", "-y", pkg])
                    .run()
                {
                    failed.push(format!("{}: {e:#}", pin.name));
                }
            }
            Install::Apt(pkg) => println!(
                "{}: not installed automatically; run `sudo apt-get install -y {pkg}` or pass --apt",
                pin.name
            ),
            Install::Hint(h) => println!("{}: install manually: {h}", pin.name),
        }
    }
    if failed.is_empty() {
        cmd_check(ctx)
    } else {
        bail!("installation failed:\n  {}", failed.join("\n  "))
    }
}
