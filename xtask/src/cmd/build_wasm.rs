//! `cargo xtask build-wasm`: the WASM build pipeline (Plan/02 §4.3, WP-CORE-06).
//!
//! Release (default):
//! 1. `cargo build -p flyff-wasm --target wasm32-unknown-unknown --profile wasm-release --locked`
//!    (paths remapped so the binary does not leak the build machine),
//! 2. the `wasm-bindgen` CLI must be exactly the version `Cargo.lock` resolves (else: install hint),
//! 3. `wasm-bindgen --target web --out-dir web/src/wasm/pkg --out-name flyff_wasm`,
//! 4. `wasm-opt` with the feature list of Rust's default wasm32 target (never SIMD),
//! 5. the generated `flyff_wasm.d.ts` must equal the committed snapshot
//!    `crates/flyff-wasm/api/flyff_wasm.d.ts` (contract `wasm-api`) unless `--accept-api`,
//! 6. a size report (raw and gzip -9).
//!
//! `--dev`: dev profile with the panic hook, `wasm-bindgen --debug --keep-debug`, no wasm-opt, no API
//! comparison. `--watch`: rebuilds `--dev` when a source of the wasm crate closure changes (300 ms
//! debounce).

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::build_id;
use crate::meta::Meta;
use crate::proc::{Ctx, Run, banner};
use anyhow::{Context, Result, bail};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Arguments of `cargo xtask build-wasm`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// `--dev`: dev profile with the panic hook, no wasm-opt.
    #[arg(long)]
    pub dev: bool,
    /// `--watch`: rebuild on change.
    #[arg(long)]
    pub watch: bool,
    /// `--accept-api`: accept a changed `flyff_wasm.d.ts` snapshot.
    #[arg(long)]
    pub accept_api: bool,
}

/// The wasm crate.
const CRATE: &str = "flyff-wasm";
/// Output directory (git-ignored).
pub const PKG_DIR: &str = "web/src/wasm/pkg";
/// Committed API snapshot (contract `wasm-api`).
const API_SNAPSHOT: &str = "crates/flyff-wasm/api/flyff_wasm.d.ts";
/// Marker written next to the glue so other commands know which profile produced the files.
const BUILD_MARKER: &str = ".flyff-build.json";
/// Optimisation flags of `wasm-opt`: the default wasm32 feature set of Rust, never `--enable-simd`
/// (D4). The level can be overridden with `FLYFF_WASM_OPT` (e.g. `-Oz`).
const WASM_OPT_FEATURES: &[&str] = &[
    "--enable-bulk-memory",
    "--enable-mutable-globals",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-reference-types",
    "--enable-multivalue",
];
const WASM_OPT_LEVEL: &str = "-O3";

/// Entry point.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    if args.watch {
        return watch(ctx);
    }
    build_pkg(ctx, args.dev, args.accept_api)
}

/// `true` when `web/src/wasm/pkg` holds a release build that is newer than every source file of the
/// wasm crate closure (so `build-web` can skip the wasm build).
pub fn pkg_is_current_release(ctx: &Ctx) -> bool {
    pkg_is_current(ctx, PKG_DIR, "release")
}

/// The marker written next to the glue: `{"profile": "<profile>"}`.
fn marker_text(profile: &str) -> String {
    format!("{{\n  \"profile\": \"{profile}\"\n}}\n")
}

fn pkg_is_current(ctx: &Ctx, dir: &str, profile: &str) -> bool {
    let pkg = ctx.path(dir);
    let glue = pkg.join("flyff_wasm.js");
    let wasm = pkg.join("flyff_wasm_bg.wasm");
    let Ok(marker) = std::fs::read_to_string(pkg.join(BUILD_MARKER)) else {
        return false;
    };
    if !marker.contains(&format!("\"profile\": \"{profile}\"")) {
        return false;
    }
    let Some(built) = [&glue, &wasm]
        .iter()
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()))
        .collect::<std::io::Result<Vec<_>>>()
        .ok()
        .and_then(|v| v.into_iter().min())
    else {
        return false;
    };
    match newest_source(ctx) {
        Ok(Some(newest)) => newest <= built,
        _ => false,
    }
}

/// The newest modification time among the sources of the wasm crate closure (`src/`, `Cargo.toml`, the
/// workspace `Cargo.lock`).
fn newest_source(ctx: &Ctx) -> Result<Option<SystemTime>> {
    let mut newest: Option<SystemTime> = None;
    let mut consider = |p: &Path| {
        if let Ok(t) = std::fs::metadata(p).and_then(|m| m.modified()) {
            newest = Some(newest.map_or(t, |n| n.max(t)));
        }
    };
    consider(&ctx.path("Cargo.lock"));
    for (dir, files) in closure_sources(ctx)? {
        for f in files {
            consider(&f);
        }
        for e in walkdir::WalkDir::new(&dir)
            .into_iter()
            .filter_map(Result::ok)
        {
            if e.file_type().is_file() {
                consider(e.path());
            }
        }
    }
    Ok(newest)
}

/// `(src dir, [manifest])` of every workspace crate in the normal-dependency closure of flyff-wasm.
fn closure_sources(ctx: &Ctx) -> Result<Vec<(PathBuf, Vec<PathBuf>)>> {
    let meta = Meta::load(ctx, Some("wasm32-unknown-unknown"), false)?;
    let names = meta.normal_closure(CRATE);
    let mut out = Vec::new();
    for p in meta.members() {
        if !names.contains(&p.name) {
            continue;
        }
        let manifest = PathBuf::from(&p.manifest_path);
        let Some(dir) = manifest.parent() else {
            continue;
        };
        out.push((dir.join("src"), vec![manifest.clone()]));
    }
    Ok(out)
}

fn build_pkg(ctx: &Ctx, dev: bool, accept_api: bool) -> Result<()> {
    let total = if dev { 3 } else { 6 };
    banner(
        "build-wasm",
        1,
        total,
        "cargo build (wasm32-unknown-unknown)",
    );
    let wasm = cargo_build(ctx, dev)?;

    banner("build-wasm", 2, total, "wasm-bindgen --target web");
    check_wasm_bindgen_version(ctx)?;
    let pkg = ctx.path(PKG_DIR);
    reset_dir(&pkg)?;
    let mut bindgen = Run::new(ctx, "wasm-bindgen").args(["--target", "web", "--out-dir"]);
    bindgen = bindgen.arg(&pkg).args(["--out-name", "flyff_wasm"]);
    if dev {
        bindgen = bindgen.args(["--debug", "--keep-debug"]);
    }
    bindgen.arg(&wasm).run()?;
    let bg = pkg.join("flyff_wasm_bg.wasm");
    if !bg.exists() {
        bail!("wasm-bindgen did not write {}", bg.display());
    }

    if dev {
        write_marker(&pkg, "dev")?;
        banner(
            "build-wasm",
            3,
            total,
            "done (dev build: no wasm-opt, no API check)",
        );
        report_sizes(&pkg)?;
        return Ok(());
    }

    banner("build-wasm", 3, total, "wasm-opt");
    let level = std::env::var("FLYFF_WASM_OPT")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| WASM_OPT_LEVEL.to_owned());
    let before = std::fs::metadata(&bg)?.len();
    Run::new(ctx, "wasm-opt")
        .arg(&level)
        .args(WASM_OPT_FEATURES)
        .arg("-o")
        .arg(&bg)
        .arg(&bg)
        .run()
        .context("wasm-opt failed (install binaryen: cargo xtask tools install --apt)")?;
    let after = std::fs::metadata(&bg)?.len();
    println!("    wasm-opt {level}: {before} -> {after} bytes");

    banner("build-wasm", 4, total, "API snapshot");
    compare_api(ctx, &pkg, accept_api)?;

    write_marker(&pkg, "release")?;
    banner("build-wasm", 5, total, "size report");
    report_sizes(&pkg)?;
    banner("build-wasm", 6, total, "done");
    println!("build-wasm: OK ({})", pkg.display());
    Ok(())
}

/// Cargo arguments for the development or release WASM build.
fn cargo_args(dev: bool) -> Vec<String> {
    let profile = if dev { "dev" } else { "wasm-release" };
    let mut args: Vec<String> = [
        "build",
        "-p",
        CRATE,
        "--target",
        "wasm32-unknown-unknown",
        "--profile",
        profile,
        "--locked",
        "--message-format=json-render-diagnostics",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let mut features = Vec::new();
    if dev {
        features.push("dev-panic-hook");
    }
    if !features.is_empty() {
        args.push("--features".to_owned());
        args.push(features.join(","));
    }
    args
}

/// Runs the cargo build and returns the produced `.wasm` file.
fn cargo_build(ctx: &Ctx, dev: bool) -> Result<PathBuf> {
    let mut run = Run::new(ctx, "cargo").args(cargo_args(dev));
    if !dev {
        let cargo_home = std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()));
        // CARGO_ENCODED_RUSTFLAGS separates flags with 0x1f.
        run = run.env(
            "CARGO_ENCODED_RUSTFLAGS",
            format!(
                "--remap-path-prefix={}=/flyff\u{1f}--remap-path-prefix={cargo_home}=/cargo",
                ctx.root.display()
            ),
        );
    }
    let lines = run.stream_stdout()?;
    find_wasm_artifact(&lines).context("cargo did not report the flyff_wasm.wasm artifact")
}

/// Extracts the `.wasm` file of the `flyff_wasm` crate from `--message-format=json` output.
fn find_wasm_artifact(lines: &[String]) -> Option<PathBuf> {
    lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == "flyff_wasm")
        .flat_map(|v| {
            v["filenames"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_str().map(PathBuf::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .rfind(|p| p.extension().is_some_and(|e| e == "wasm"))
}

/// The `wasm-bindgen` CLI must match the library version of `Cargo.lock` exactly.
fn check_wasm_bindgen_version(ctx: &Ctx) -> Result<()> {
    let want = build_id::repo_lock_version(ctx, "wasm-bindgen")?;
    let out = Run::new(ctx, "wasm-bindgen")
        .arg("--version")
        .output()
        .context("wasm-bindgen CLI not found: run `cargo xtask tools install`")?;
    let found = out.split_whitespace().nth(1).unwrap_or_default().to_owned();
    if found != want {
        bail!(
            "wasm-bindgen CLI is {found} but Cargo.lock pins wasm-bindgen {want}; run `cargo xtask tools install` (installs wasm-bindgen-cli {want} into .tools/)"
        );
    }
    Ok(())
}

fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).with_context(|| format!("remove {}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))
}

fn write_marker(pkg: &Path, profile: &str) -> Result<()> {
    std::fs::write(pkg.join(BUILD_MARKER), marker_text(profile))?;
    Ok(())
}

/// Compares the generated `.d.ts` with the committed snapshot (contract `wasm-api`).
fn compare_api(ctx: &Ctx, pkg: &Path, accept: bool) -> Result<()> {
    let generated = pkg.join("flyff_wasm.d.ts");
    let snapshot = ctx.path(API_SNAPSHOT);
    let new = std::fs::read(&generated).context("read the generated flyff_wasm.d.ts")?;
    let old = std::fs::read(&snapshot).ok();
    if old.as_deref() == Some(new.as_slice()) {
        println!("    API snapshot unchanged ({})", snapshot.display());
        return Ok(());
    }
    if accept {
        if let Some(parent) = snapshot.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&snapshot, &new)?;
        println!(
            "    --accept-api: wrote {} (commit it; the contract `wasm-api` guard reviews the change)",
            snapshot.display()
        );
        return Ok(());
    }
    match old {
        None => eprintln!("    no API snapshot yet at {}", snapshot.display()),
        Some(_) => {
            if let Ok((_, diff, _)) = Run::new(ctx, "diff")
                .args(["-u"])
                .arg(&snapshot)
                .arg(&generated)
                .capture()
            {
                eprintln!("{diff}");
            }
        }
    }
    bail!(
        "the exported wasm API changed; review the diff above and re-run with `cargo xtask build-wasm --accept-api` to accept it"
    )
}

fn gzip_len(bytes: &[u8]) -> Result<usize> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(bytes)?;
    Ok(enc.finish()?.len())
}

fn report_sizes(pkg: &Path) -> Result<()> {
    for name in ["flyff_wasm_bg.wasm", "flyff_wasm.js"] {
        let bytes = std::fs::read(pkg.join(name))?;
        println!(
            "    {name}: {} bytes raw, {} bytes gzip -9",
            bytes.len(),
            gzip_len(&bytes)?
        );
    }
    Ok(())
}

// ---- --watch --------------------------------------------------------------------------------------

fn watch(ctx: &Ctx) -> Result<()> {
    use notify::{RecursiveMode, Watcher};
    banner("build-wasm", 1, 2, "initial dev build");
    if let Err(e) = build_pkg(ctx, true, false) {
        eprintln!("build-wasm: {e:#}");
    }
    banner("build-wasm", 2, 2, "watching (Ctrl-C stops)");
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let _ = tx.send(res);
    })
    .context("start the file watcher")?;
    let mut count = 0;
    for (src, manifests) in closure_sources(ctx)? {
        if src.is_dir() {
            watcher.watch(&src, RecursiveMode::Recursive)?;
            count += 1;
        }
        for m in manifests {
            watcher.watch(&m, RecursiveMode::NonRecursive)?;
        }
    }
    println!("    watching {count} source directories");
    loop {
        // wait for the first relevant event, then let the burst settle (300 ms debounce)
        match rx.recv() {
            Ok(Ok(ev)) if is_relevant(&ev) => {}
            Ok(_) => continue,
            Err(_) => return Ok(()),
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(300)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        println!("==> source change: rebuilding the wasm module (dev)");
        if let Err(e) = build_pkg(ctx, true, false) {
            eprintln!("build-wasm: {e:#}");
        }
    }
}

fn is_relevant(ev: &notify::Event) -> bool {
    use notify::EventKind;
    matches!(
        ev.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && ev.paths.iter().any(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        !(name.starts_with('.') || name.ends_with('~') || name.ends_with(".swp"))
    })
}
