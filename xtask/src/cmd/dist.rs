//! `cargo xtask dist`: builds the release package (Plan/02 §8.1, WP-OPS-55 subset).
//!
//! Steps: preflight, `gen --check`, `build-wasm`, `build-web`, build the static musl `flyff` with the
//! embedded web client, verify the ELF (static, no `PT_INTERP`, no `DT_NEEDED`) and `version`, assemble
//! `<out>/` (flyff, README.txt, flyff.toml.example, flyff.service.example, scripts/npc/, VERSION,
//! LICENSES/) and optionally copy the original `Resource/` (never hard links; `*.exe *.dll *.sys *.asi
//! *.lib` excluded), write a tarball.
//!
//! The assembly only replaces the files it manages: `flyff.toml`, `data/`, `cache/`, `backups/`,
//! `logs/` and an existing `Resource/` are never deleted, so running `dist` into a live install is safe.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::build_id;
use crate::cmd::{build_wasm, build_web, gen_lint, gen_proto};
use crate::meta::Meta;
use crate::proc::{Ctx, Run, banner};
use crate::tools;
use anyhow::{Context, Result, bail};
use object::read::elf::{Dyn, FileHeader, ProgramHeader};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Arguments of `cargo xtask dist`.
#[derive(clap::Args, Debug, Clone)]
pub struct Args {
    /// Output directory (default: `<repo>/dist/flyff`).
    #[arg(long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// The original `Resource/` folder (default: `$FLYFF_RESOURCE`); used by --copy-resource and --prepare.
    #[arg(long, value_name = "DIR")]
    pub resource: Option<PathBuf>,
    /// Copy the Resource folder into `<out>/Resource` (plain copy, no hard links; excludes
    /// *.exe *.dll *.sys *.asi *.lib).
    #[arg(long)]
    pub copy_resource: bool,
    /// Do not build the web client and the wasm module (embed the existing `web/dist` or a placeholder).
    #[arg(long)]
    pub skip_web: bool,
    /// Rust target triple.
    #[arg(long, default_value = "x86_64-unknown-linux-musl")]
    pub target: String,
    /// Run `<out>/flyff prepare` against the Resource folder so that `<out>/cache/` is filled (valid only for that
    /// Resource copy: the cache fingerprints include file mtimes).
    #[arg(long)]
    pub prepare: bool,
    /// Also write `<out>/../flyff-<version>-linux-x86_64.tar.gz`.
    #[arg(long)]
    pub tar: bool,
    /// Fail on a dirty git tree (tagged releases).
    #[arg(long)]
    pub require_clean: bool,
}

// ------------------------------------------------------------------------------------------------
// web embedding
// ------------------------------------------------------------------------------------------------

const PLACEHOLDER_INDEX: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Flyff V15</title></head>\n<body><h1>Flyff V15</h1><p>The web client was not built into this package.</p></body></html>\n";

/// A generated stand-in for `web/dist` (rust-embed needs an existing folder at compile time).
pub fn ensure_placeholder_web(ctx: &Ctx) -> Result<PathBuf> {
    let meta = Meta::load(ctx, None, true)?;
    let dir = PathBuf::from(meta.target_directory).join("placeholder-web");
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let index = dir.join("index.html");
    if std::fs::read_to_string(&index).ok().as_deref() != Some(PLACEHOLDER_INDEX) {
        std::fs::write(&index, PLACEHOLDER_INDEX)?;
    }
    Ok(dir)
}

/// SHA-256 over the sorted relative paths and contents of `dir` (hex, first 16 chars).
pub fn dir_fingerprint(dir: &Path) -> Result<String> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .collect();
    files.sort();
    let mut h = Sha256::new();
    for f in files {
        let rel = f
            .strip_prefix(dir)
            .unwrap_or(&f)
            .to_string_lossy()
            .into_owned();
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(std::fs::read(&f).with_context(|| format!("read {}", f.display()))?);
        h.update([0]);
    }
    Ok(hex(&h.finalize())[..16].to_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ------------------------------------------------------------------------------------------------
// ELF verification (Plan/02 §6)
// ------------------------------------------------------------------------------------------------

/// What the ELF check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfReport {
    /// `e_machine == EM_X86_64`.
    pub x86_64: bool,
    /// A `PT_INTERP` program header exists (dynamically linked against a loader).
    pub has_interp: bool,
    /// Number of `DT_NEEDED` entries.
    pub needed: usize,
}

impl ElfReport {
    /// Fully static: x86-64, no interpreter, no needed libraries.
    pub fn is_static(&self) -> bool {
        self.x86_64 && !self.has_interp && self.needed == 0
    }
}

/// Inspects an ELF64 little-endian image.
pub fn inspect_elf(bytes: &[u8]) -> Result<ElfReport> {
    use object::elf::{DT_NEEDED, EM_X86_64, FileHeader64, PT_INTERP};
    let header = FileHeader64::<object::Endianness>::parse(bytes)
        .map_err(|e| anyhow::anyhow!("not an ELF64 file: {e}"))?;
    let endian = header
        .endian()
        .map_err(|e| anyhow::anyhow!("bad ELF endianness: {e}"))?;
    let x86_64 = header.e_machine(endian) == EM_X86_64;
    let segments = header
        .program_headers(endian, bytes)
        .map_err(|e| anyhow::anyhow!("bad program headers: {e}"))?;
    let mut has_interp = false;
    let mut needed = 0;
    for seg in segments {
        if seg.p_type(endian) == PT_INTERP {
            has_interp = true;
        }
        if let Ok(Some(dynamic)) = seg.dynamic(endian, bytes) {
            needed += dynamic
                .iter()
                .filter(|d| d.d_tag(endian) == DT_NEEDED)
                .count();
        }
    }
    Ok(ElfReport {
        x86_64,
        has_interp,
        needed,
    })
}

// ------------------------------------------------------------------------------------------------
// Resource copy
// ------------------------------------------------------------------------------------------------

/// Extensions never copied from `Resource/` (Windows binaries of the original client).
pub const EXCLUDED_EXTENSIONS: &[&str] = &["exe", "dll", "sys", "asi", "lib"];

/// `true` for files that must not be copied.
pub fn is_excluded(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        EXCLUDED_EXTENSIONS
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// Result of [`copy_resource`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CopyReport {
    /// Files written.
    pub copied: u64,
    /// Files already up to date.
    pub unchanged: u64,
    /// Files skipped by the extension filter.
    pub excluded: u64,
    /// Bytes written.
    pub bytes: u64,
}

/// Plain recursive copy (never hard links) with the extension filter; unchanged files (same size and a
/// destination at least as new) are skipped so the copy is incremental.
pub fn copy_resource(src: &Path, dst: &Path) -> Result<CopyReport> {
    let mut report = CopyReport::default();
    std::fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for entry in walkdir::WalkDir::new(src).sort_by_file_name() {
        let entry = entry.with_context(|| format!("walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src).unwrap_or(entry.path());
        let target = dst.join(rel);
        let ft = entry.file_type();
        if ft.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        // Symlinked files are copied by content; symlinked directories are not followed.
        let meta = match std::fs::metadata(entry.path()) {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        if is_excluded(entry.path()) {
            report.excluded += 1;
            continue;
        }
        if let Ok(existing) = std::fs::metadata(&target) {
            let fresh = existing.len() == meta.len()
                && matches!((existing.modified(), meta.modified()), (Ok(d), Ok(s)) if d >= s);
            if fresh {
                report.unchanged += 1;
                continue;
            }
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // `fs::copy` creates a new inode and copies the data: no hard link, no symlink.
        let n = std::fs::copy(entry.path(), &target)
            .with_context(|| format!("copy {}", entry.path().display()))?;
        report.copied += 1;
        report.bytes += n;
        if (report.copied + report.unchanged) % 2000 == 0 {
            println!(
                "    ... {} files, {:.1} MiB",
                report.copied + report.unchanged,
                report.bytes as f64 / 1_048_576.0
            );
        }
    }
    Ok(report)
}

// ------------------------------------------------------------------------------------------------
// assembly
// ------------------------------------------------------------------------------------------------

fn render_template(text: &str, version: &str) -> String {
    text.replace("{{VERSION}}", version)
}

/// Copies `content/npc/*.lua` and writes `MANIFEST.txt` (`<sha256>  <file>` per file + the build id).
fn assemble_npc_scripts(ctx: &Ctx, out: &Path, build_id: &str) -> Result<usize> {
    let dst = out.join("scripts").join("npc");
    std::fs::create_dir_all(&dst)?;
    let src = ctx.path("content/npc");
    let mut manifest = Vec::new();
    if src.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(&src)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "lua"))
            .collect();
        names.sort();
        for p in names {
            let bytes = std::fs::read(&p)?;
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            std::fs::write(dst.join(&name), &bytes)?;
            manifest.push(format!("{}  {name}", hex(&Sha256::digest(&bytes))));
        }
    }
    let count = manifest.len();
    manifest.push(format!("# build {build_id}"));
    std::fs::write(dst.join("MANIFEST.txt"), manifest.join("\n") + "\n")?;
    Ok(count)
}

/// `LICENSES/THIRD-PARTY-RUST.txt` from `cargo metadata` of the dist graph.
fn write_licenses(ctx: &Ctx, out: &Path, target: &str) -> Result<usize> {
    let meta = Meta::load(ctx, Some(target), false)?;
    let names = meta.normal_closure("flyff-server");
    let mut rows: Vec<String> = meta
        .packages
        .iter()
        .filter(|p| names.contains(&p.name) && !p.name.starts_with("flyff-"))
        .map(|p| {
            format!(
                "{} {}  --  {}",
                p.name,
                p.version,
                p.license.clone().unwrap_or_else(|| "see crate".to_owned())
            )
        })
        .collect();
    rows.sort();
    rows.dedup();
    let dir = out.join("LICENSES");
    std::fs::create_dir_all(&dir)?;
    let mut f = std::fs::File::create(dir.join("THIRD-PARTY-RUST.txt"))?;
    writeln!(
        f,
        "Third-party Rust crates linked into the flyff executable ({} crates)\n",
        rows.len()
    )?;
    for r in &rows {
        writeln!(f, "{r}")?;
    }
    Ok(rows.len())
}

// ------------------------------------------------------------------------------------------------
// main entry
// ------------------------------------------------------------------------------------------------

/// Runs `cargo xtask dist`.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    // Absolute paths: child processes run in the repository root, not in the caller's directory.
    let out = std::path::absolute(args.out.clone().unwrap_or_else(|| ctx.path("dist/flyff")))
        .context("resolve the output directory")?;
    let total =
        7 + usize::from(args.copy_resource) + usize::from(args.prepare) + usize::from(args.tar);
    let mut n = 0;
    let mut step = |name: &str| {
        n += 1;
        banner("dist", n, total, name);
    };

    step("preflight");
    let rows = tools::check_all(ctx);
    let required_ok = rows
        .iter()
        .filter(|r| r.required)
        .all(|r| r.status == tools::Status::Ok);
    if !required_ok {
        tools::print_table(&rows);
        bail!("required tools are missing (see `cargo xtask tools check`)");
    }
    let id = build_id::compute(ctx)?;
    println!(
        "    build id {id}, target {}, output {}",
        args.target,
        out.display()
    );
    if id.ends_with(".dirty") {
        if args.require_clean {
            bail!("the git tree is dirty and --require-clean was given");
        }
        println!("    warning: building from a dirty tree");
    }
    let resource = args.resource.clone().or_else(|| {
        std::env::var_os("FLYFF_RESOURCE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    });

    step("gen --check");
    gen_lint::run(ctx, true)?;
    gen_proto::gen_if_available(ctx, true)?;

    let web_present = ctx.path("web/package.json").exists();
    step("build-wasm");
    if args.skip_web || !web_present {
        println!(
            "    skipped ({})",
            if args.skip_web {
                "--skip-web"
            } else {
                "web/ does not exist yet"
            }
        );
    } else {
        build_wasm::run(ctx, &build_wasm::Args::default())?;
    }
    step("build-web");
    if args.skip_web || !web_present {
        println!(
            "    skipped ({})",
            if args.skip_web {
                "--skip-web"
            } else {
                "web/ does not exist yet"
            }
        );
    } else {
        build_web::run(ctx, &build_web::Args::default())?;
    }

    // The folder rust-embed embeds (compile-time): the real build when it exists, else a placeholder.
    let real_web = ctx.path("web/dist");
    let (web_dir, web_kind) = if real_web.join("index.html").exists() {
        (real_web, "web/dist")
    } else {
        (ensure_placeholder_web(ctx)?, "placeholder page")
    };
    let fingerprint = dir_fingerprint(&web_dir)?;
    println!(
        "    embedding {web_kind} ({}, fingerprint {fingerprint})",
        web_dir.display()
    );

    step("build the static server");
    let remap = format!(
        "--remap-path-prefix={}=/flyff\u{1f}--remap-path-prefix={}=/cargo",
        ctx.root.display(),
        std::env::var("CARGO_HOME")
            .unwrap_or_else(|_| format!("{}/.cargo", std::env::var("HOME").unwrap_or_default()))
    );
    let lines = Run::new(ctx, "cargo")
        .args([
            "build",
            "-p",
            "flyff-server",
            "--bin",
            "flyff",
            "--profile",
            "dist",
            "--features",
            "embed-web",
            "--locked",
            "--target",
        ])
        .arg(&args.target)
        .arg("--message-format=json-render-diagnostics")
        .env("FLYFF_BUILD_ID", &id)
        .env("FLYFF_TARGET", &args.target)
        .env("FLYFF_PROFILE", "dist")
        .env("FLYFF_WEB_DIST_DIR", &web_dir)
        .env("FLYFF_WEB_FINGERPRINT", &fingerprint)
        .env("CARGO_ENCODED_RUSTFLAGS", remap)
        .stream_stdout()?;
    let binary = lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| {
            v["reason"] == "compiler-artifact"
                && v["target"]["name"] == "flyff"
                && v["executable"].is_string()
        })
        .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
        .next_back()
        .context("cargo did not report the flyff executable")?;

    step("verify the binary");
    let bytes = std::fs::read(&binary).with_context(|| format!("read {}", binary.display()))?;
    let report = inspect_elf(&bytes)?;
    println!(
        "    {} ({:.1} MiB): x86_64={}, PT_INTERP={}, DT_NEEDED={}",
        binary.display(),
        bytes.len() as f64 / 1_048_576.0,
        report.x86_64,
        report.has_interp,
        report.needed
    );
    if args.target.contains("musl") && !report.is_static() {
        bail!(
            "the binary is not fully static (PT_INTERP={}, DT_NEEDED={})",
            report.has_interp,
            report.needed
        );
    }
    if let Ok(ldd) = Run::new(ctx, "ldd").arg(&binary).capture() {
        println!(
            "    ldd: {}",
            format!("{}{}", ldd.1, ldd.2).trim().replace('\n', " | ")
        );
    }
    let version_out = std::process::Command::new(&binary)
        .arg("version")
        .output()
        .context("run `flyff version`")?;
    let version_line = String::from_utf8_lossy(&version_out.stdout)
        .trim()
        .to_owned();
    println!("    {version_line}");
    if !version_out.status.success() || !version_line.contains(&id) {
        bail!("`flyff version` did not print the expected build id {id}: {version_line:?}");
    }
    // The size is reported above; the 40 MiB budget of Plan/02 §8.8 was
    // dropped by the owner on 2026-10-29.

    step("assemble the package");
    std::fs::create_dir_all(&out).with_context(|| format!("create {}", out.display()))?;
    let dest_bin = out.join("flyff");
    let tmp_bin = out.join(".flyff.new");
    std::fs::copy(&binary, &tmp_bin)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_bin, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp_bin, &dest_bin)?;
    let ws_version = build_id::workspace_version(ctx)?;
    std::fs::write(
        out.join("README.txt"),
        render_template(include_str!("../../templates/dist/README.txt"), &id),
    )?;
    std::fs::write(
        out.join("flyff.service.example"),
        include_str!("../../templates/dist/flyff.service.example"),
    )?;
    std::fs::copy(
        ctx.path("crates/flyff-server/assets/flyff.toml.example"),
        out.join("flyff.toml.example"),
    )?;
    let npc = assemble_npc_scripts(ctx, &out, &id)?;
    let guide = copy_admin_guide(ctx, &out)?;
    let proto = version_line
        .split("proto ")
        .nth(1)
        .and_then(|s| s.split([',', ')']).next())
        .unwrap_or("unknown")
        .to_owned();
    std::fs::write(
        out.join("VERSION"),
        format!("{id}\n{proto}\n{}\n", args.target),
    )?;
    let crates = write_licenses(ctx, &out, &args.target)?;
    println!(
        "    {} written: flyff, README.txt, flyff.toml.example, flyff.service.example, scripts/npc ({npc} scripts), docs/ ({guide} admin guide files), VERSION, LICENSES ({crates} crates; v{ws_version})",
        out.display()
    );

    if args.copy_resource {
        step("copy the Resource folder");
        let Some(res) = resource.as_deref() else {
            bail!("--copy-resource needs --resource DIR or FLYFF_RESOURCE")
        };
        if !crate::cmd::doctor::classify_tree(Some(res.as_os_str()), "Masquerade.prj")
            .eq(&crate::cmd::doctor::TreeState::Valid)
        {
            bail!(
                "{} is not a Resource folder (no Masquerade.prj)",
                res.display()
            );
        }
        println!(
            "    copying {} -> {} ...",
            res.display(),
            out.join("Resource").display()
        );
        let r = copy_resource(res, &out.join("Resource"))?;
        println!(
            "    Resource: {} files copied ({:.1} MiB), {} unchanged, {} excluded (*.exe *.dll *.sys *.asi *.lib)",
            r.copied,
            r.bytes as f64 / 1_048_576.0,
            r.unchanged,
            r.excluded
        );
    }

    if args.prepare {
        step("prepare the derived assets");
        let packaged = out.join("Resource");
        let res = if args.copy_resource {
            Some(packaged.as_path())
        } else {
            resource.as_deref()
        };
        let Some(res) = res else {
            bail!("--prepare needs --resource DIR (or FLYFF_RESOURCE)")
        };
        let mut cmd = std::process::Command::new(&dest_bin);
        cmd.arg("prepare")
            .arg("--home")
            .arg(&out)
            .arg("--resource")
            .arg(res);
        println!(
            "$ {} prepare --home {} --resource {}",
            dest_bin.display(),
            out.display(),
            res.display()
        );
        let status = cmd.status().context("run `flyff prepare`")?;
        if !status.success() {
            bail!("`flyff prepare` failed with {status}");
        }
    }

    if args.tar {
        step("tarball");
        let parent = out.parent().context("output directory has no parent")?;
        let name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "flyff".to_owned());
        let tarball = parent.join(format!("flyff-{ws_version}-linux-x86_64.tar.gz"));
        Run::new(ctx, "tar")
            .args([
                "--sort=name",
                "--owner=0",
                "--group=0",
                "--numeric-owner",
                "-czf",
            ])
            .arg(&tarball)
            .arg("-C")
            .arg(parent)
            .arg(&name)
            .run()?;
        let sum = hex(&Sha256::digest(std::fs::read(&tarball)?));
        std::fs::write(
            tarball.with_extension("gz.sha256"),
            format!(
                "{sum}  {}\n",
                tarball
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
        )?;
        println!("    {} ({sum})", tarball.display());
    }
    println!("dist: OK -> {}", out.display());
    Ok(())
}

/// The operator guide of the admin console (`docs/admin-console.md`, Plan/25 WP-ADMIN-32) and its screenshots
/// (`docs/shots/admin-guide-*.png`) go into the package as `docs/`, so the relative image paths of the guide keep working and the
/// "?" help of the console and `README.txt` can name a file that is really there. Returns the number of files copied.
fn copy_admin_guide(ctx: &Ctx, out: &Path) -> Result<usize> {
    let guide = ctx.path("docs/admin-console.md");
    if !guide.is_file() {
        return Ok(0);
    }
    let docs = out.join("docs");
    std::fs::create_dir_all(docs.join("shots"))?;
    std::fs::copy(&guide, docs.join("admin-console.md"))?;
    let mut copied = 1;
    let shots = ctx.path("docs/shots");
    if shots.is_dir() {
        for entry in std::fs::read_dir(&shots)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("admin-guide-") && name.ends_with(".png") {
                std::fs::copy(entry.path(), docs.join("shots").join(&name))?;
                copied += 1;
            }
        }
    }
    Ok(copied)
}
