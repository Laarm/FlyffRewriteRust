//! `cargo xtask build-web`: the production web build (Plan/02 §4.4, WP-CORE-04, WP-CLIENT-01).
//!
//! 1. `pnpm --dir web install --frozen-lockfile` (skipped with `--skip-install`),
//! 2. the wasm module `web/src/wasm/pkg/flyff_wasm.js` must exist; a missing, dev-profile or stale
//!    package is rebuilt with `build-wasm` first (skip with `--skip-wasm`).
//! 3. `pnpm --dir web run typecheck`, then `pnpm --dir web run build` with `FLYFF_BUILD_ID` into
//!    `web/dist/`,
//! 4. `web/dist/build-id.txt`,
//! 5. `.gz` siblings for `html js mjs css wasm json svg txt` files of at least 1 KiB, written only when
//!    `gz/raw < 0.9` (D6),
//! 6. the size report (Plan/40 §16, Plan/02 §8.8, WP-CLIENT-17, Plan/25 §11.2, §13, WP-ADMIN-03): initial JS, boot
//!    path, the lazy render chunk, wasm, the player client, the 02 §8.8 size targets (reported only, never a
//!    failure: the budgets were made informational by the owner on 2026-10-29) and the isolation checks: no dev
//!    surface and no `src/admin/` module is reachable from `index.html` (always a failure); the admin console has its
//!    own size targets (entry JS 600 KB gzip excluding the chunks the player client shares, map chunk 250 KB gzip,
//!    reported only, like a map engine inside the entry chunk) and its files are reported apart from the player's 8 MB,
//! 7. the dist-shape check: nothing but `assets/` may be a directory (Plan/24 serves `/assets/*` and
//!    single-segment root files only), `index.html` and `admin.html` must exist, `index.html` must keep
//!    `%FLYFF_TITLE%`, the fallback font must be a hashed file under `assets/`, and no source maps are shipped.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use crate::build_id;
use crate::cmd::build_wasm;
use crate::proc::{Ctx, Run, banner};
use anyhow::{Context, Result, bail};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Arguments of `cargo xtask build-web`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// `--skip-install`: do not run `pnpm install`.
    #[arg(long)]
    pub skip_install: bool,
    /// `--skip-wasm`: never build the wasm module (it must already exist).
    #[arg(long)]
    pub skip_wasm: bool,
    /// `--out-dir <dir>`: build into this directory instead of `web/dist` (the e2e runs build a private copy so
    /// they never touch the `web/dist` other tasks serve).
    #[arg(long)]
    pub out_dir: Option<PathBuf>,
}

/// Extensions that get a `.gz` sibling.
const GZIP_EXTENSIONS: &[&str] = &["html", "js", "mjs", "css", "wasm", "json", "svg", "txt"];
/// Files smaller than this are never precompressed.
const GZIP_MIN_BYTES: usize = 1024;
/// A `.gz` sibling is kept only when it is smaller than this fraction of the original (D6).
const GZIP_MAX_RATIO: f64 = 0.9;

/// Entry point.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    let web = ctx.path("web");
    if !web.join("package.json").exists() {
        bail!("web/package.json does not exist");
    }
    let total = 7;

    banner("build-web", 1, total, "pnpm install --frozen-lockfile");
    if args.skip_install {
        println!("    skipped (--skip-install)");
    } else {
        Run::new(ctx, "pnpm")
            .args(["--dir", "web", "install", "--frozen-lockfile"])
            .run()
            .context("pnpm install failed (Node 24 and pnpm 12 via corepack: see README)")?;
    }

    banner("build-web", 2, total, "wasm module");
    let glue = ctx.path(build_wasm::PKG_DIR).join("flyff_wasm.js");
    if args.skip_wasm {
        if !glue.exists() {
            bail!(
                "{} does not exist: run `cargo xtask build-wasm`",
                glue.display()
            );
        }
        println!("    skipped (--skip-wasm)");
    } else if build_wasm::pkg_is_current_release(ctx) {
        println!("    web/src/wasm/pkg is a current release build");
    } else {
        build_wasm::run(ctx, &build_wasm::Args::default())?;
    }
    if !glue.exists() {
        bail!(
            "{} does not exist: run `cargo xtask build-wasm`",
            glue.display()
        );
    }
    banner("build-web", 3, total, "typecheck and vite build");
    let id = match std::env::var("FLYFF_BUILD_ID") {
        Ok(v) if !v.is_empty() => v,
        _ => build_id::compute(ctx)?,
    };
    Run::new(ctx, "pnpm")
        .args(["--dir", "web", "run", "typecheck"])
        .run()?;
    // `pnpm --dir web` runs vite inside `web/`: a relative `--out-dir` is made absolute against the caller's directory first
    // (it used to land in `web/<dir>` while the rest of this command looked for it in `<dir>`)
    let dist = match &args.out_dir {
        Some(dir) => std::path::absolute(dir)
            .with_context(|| format!("absolute path of {}", dir.display()))?,
        None => web.join("dist"),
    };
    let mut build = Run::new(ctx, "pnpm").args(["--dir", "web", "run", "build"]);
    if args.out_dir.is_some() {
        build = build.arg("--outDir").arg(&dist).arg("--emptyOutDir");
    }
    build.env("FLYFF_BUILD_ID", &id).run()?;

    banner("build-web", 4, total, "build-id.txt");
    std::fs::write(dist.join("build-id.txt"), format!("{id}\n")).context("write build-id.txt")?;
    println!("    {id}");

    banner("build-web", 5, total, "gzip siblings");
    let (kept, scanned) = write_gzip_siblings(&dist)?;
    println!("    {kept} of {scanned} candidate files compress below {GZIP_MAX_RATIO}");

    banner("build-web", 6, total, "size report");
    let sizes = size_report(&dist)?;
    println!(
        "    summary: player client {}, admin console {} (entry {}{}), {} over a size target (informational)",
        kib(sizes.client_gz),
        kib(sizes.admin_gz),
        kib(sizes.admin_entry_gz),
        sizes
            .admin_map_gz
            .map_or_else(String::new, |m| format!(", map chunk {}", kib(m))),
        sizes.over.len()
    );

    banner("build-web", 7, total, "dist-shape check");
    check_dist_shape(&dist)?;
    println!("build-web: OK ({})", dist.display());
    Ok(())
}

/// `true` for files that may get a `.gz` sibling.
fn is_gzip_candidate(path: &Path, len: usize) -> bool {
    len >= GZIP_MIN_BYTES
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| GZIP_EXTENSIONS.contains(&e))
}

/// gzip level 9, header mtime 0 (reproducible).
fn gzip_bytes(raw: &[u8]) -> Result<Vec<u8>> {
    let mut enc = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    enc.write_all(raw)?;
    Ok(enc.finish()?)
}

/// Removes every `.gz` file, then writes `<file>.gz` where it pays off. Returns `(kept, candidates)`.
fn write_gzip_siblings(dist: &Path) -> Result<(usize, usize)> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dist).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().is_some_and(|e| e == "gz") {
            std::fs::remove_file(entry.path())?;
            continue;
        }
        files.push(entry.into_path());
    }
    let (mut kept, mut scanned) = (0, 0);
    for path in files {
        let raw = std::fs::read(&path)?;
        if !is_gzip_candidate(&path, raw.len()) {
            continue;
        }
        scanned += 1;
        let gz = gzip_bytes(&raw)?;
        if (gz.len() as f64) / (raw.len() as f64) < GZIP_MAX_RATIO {
            let mut name = path.clone().into_os_string();
            name.push(".gz");
            std::fs::write(PathBuf::from(name), gz)?;
            kept += 1;
        }
    }
    Ok((kept, scanned))
}

/// Vite's build manifest (`build.manifest = "build-manifest.json"`, removed again after the report).
const BUILD_MANIFEST: &str = "build-manifest.json";
/// Plan/02 §8.8 size targets, reported only since 2026-10-29 (owner): initial JS (the code of the boot path) in gzip.
const BUDGET_INITIAL_JS_GZ: usize = 1024 * 1024;
/// Plan/02 §8.8: the wasm after wasm-opt, raw and gzip.
const BUDGET_WASM_RAW: usize = 4 * 1024 * 1024;
const BUDGET_WASM_GZ: usize = 1536 * 1024;
/// Plan/01 §12.2 (edited by Plan/25 §0.4): the embedded player client in gzip. It counts the files the player's page
/// can reach (everything but the admin console's and the dev surfaces' own files).
const BUDGET_TOTAL_GZ: usize = 8 * 1000 * 1000;
/// Vite's module list per output file (`web/vite.config.ts` `buildModuleList`), removed again after the report.
const BUILD_MODULES: &str = "build-modules.json";
/// Plan/25 §13: the admin entry JS in gzip, excluding the chunks the player client shares (react, proto, ...).
const BUDGET_ADMIN_ENTRY_GZ: usize = 600 * 1024;
/// Plan/25 §13: the lazily loaded map chunk of the admin console in gzip.
const BUDGET_ADMIN_MAP_GZ: usize = 250 * 1024;
/// Sources of the admin console: never reachable from `index.html` (Plan/25 §11.2).
const ADMIN_SOURCES: &str = "src/admin/";
/// The map engine of the console: loaded only when the Map tab opens (Plan/25 §3.8, §14.3 #1); the report notes it when
/// it lands in the entry chunk (informational since 2026-10-29, owner).
const ADMIN_MAP_SOURCES: &str = "src/admin/world/";
/// Source prefixes that must never be reachable from `index.html` (Plan/40 §13.1, WP-CLIENT-16).
const DEV_ONLY_SOURCES: &[&str] = &[
    "src/dev/",
    "src/ui/dev/",
    "src/render/qa/",
    "src/render/preview/dev",
];

#[derive(Debug, Default, Clone)]
struct Chunk {
    file: String,
    src: Option<String>,
    imports: Vec<String>,
    dynamic_imports: Vec<String>,
    css: Vec<String>,
    /// Files the chunk references by URL (images imported with `?url`).
    assets: Vec<String>,
}

fn string_list(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `<dist>/build-manifest.json` into chunks by manifest key.
fn read_chunks(dist: &Path) -> Result<std::collections::BTreeMap<String, Chunk>> {
    let path = dist.join(BUILD_MANIFEST);
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "{} is missing: vite.config.ts must set build.manifest",
            path.display()
        )
    })?;
    let json: serde_json::Value =
        serde_json::from_str(&text).context("parse the build manifest")?;
    let obj = json
        .as_object()
        .context("the build manifest is not an object")?;
    Ok(obj
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                Chunk {
                    file: v
                        .get("file")
                        .and_then(|f| f.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    src: v.get("src").and_then(|f| f.as_str()).map(str::to_owned),
                    imports: string_list(v.get("imports")),
                    dynamic_imports: string_list(v.get("dynamicImports")),
                    css: string_list(v.get("css")),
                    assets: string_list(v.get("assets")),
                },
            )
        })
        .collect())
}

/// The manifest keys reachable from `roots` through static imports (and, with `dynamic`, dynamic ones).
fn closure(
    chunks: &std::collections::BTreeMap<String, Chunk>,
    roots: &[&str],
    dynamic: bool,
) -> std::collections::BTreeSet<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack: Vec<String> = roots.iter().map(|r| (*r).to_owned()).collect();
    while let Some(k) = stack.pop() {
        if !seen.insert(k.clone()) {
            continue;
        }
        if let Some(c) = chunks.get(&k) {
            stack.extend(c.imports.iter().cloned());
            if dynamic {
                stack.extend(c.dynamic_imports.iter().cloned());
            }
        }
    }
    seen
}

/// gzip size of a dist file: the `.gz` sibling when there is one, else a fresh gzip of the file.
fn gz_len(dist: &Path, rel: &str) -> Result<usize> {
    let mut sibling = dist.join(rel).into_os_string();
    sibling.push(".gz");
    let sibling = PathBuf::from(sibling);
    if sibling.is_file() {
        return Ok(std::fs::metadata(sibling)?.len() as usize);
    }
    Ok(gzip_bytes(&std::fs::read(dist.join(rel)).with_context(|| format!("read {rel}"))?)?.len())
}

fn kib(n: usize) -> String {
    format!("{:.1} KiB", n as f64 / 1024.0)
}

/// Output file -> the source modules inside it (`build-modules.json`), `None` when the build did not write the list.
type ModuleList = std::collections::BTreeMap<String, Vec<String>>;

fn read_modules(dist: &Path) -> Result<Option<ModuleList>> {
    let path = dist.join(BUILD_MODULES);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let json: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let obj = json
        .as_object()
        .with_context(|| format!("{} is not an object", path.display()))?;
    Ok(Some(
        obj.iter()
            .map(|(file, mods)| (file.clone(), string_list(Some(mods))))
            .collect(),
    ))
}

/// The output files that only the html entry `entry` (and the chunks no other entry reaches) loads: its chunks, their css and the
/// files they reference by URL (the guide's screenshots), and the html file itself.
fn exclusive_files(
    chunks: &std::collections::BTreeMap<String, Chunk>,
    entry: &str,
    others: &[&str],
) -> std::collections::BTreeSet<String> {
    let mine = closure(chunks, &[entry], true);
    let mut theirs = std::collections::BTreeSet::new();
    for other in others {
        theirs.extend(closure(chunks, &[other], true));
    }
    let mut files = std::collections::BTreeSet::new();
    for key in mine.difference(&theirs) {
        if let Some(c) = chunks.get(key) {
            if !c.file.is_empty() {
                files.insert(c.file.clone());
            }
            files.extend(c.css.iter().cloned());
            files.extend(c.assets.iter().cloned());
        }
    }
    if chunks.contains_key(entry) {
        files.insert(entry.to_owned());
    }
    files
}

/// What the module graph says about `src/admin/` (Plan/25 §11.2) and the map chunk's laziness.
#[derive(Debug, Default)]
struct AdminFindings {
    /// `file: module` of every `src/admin/` module inside a chunk the player's `index.html` can reach.
    leaked: Vec<String>,
    /// Map-engine modules inside a chunk of the console's entry (they must load lazily).
    eager_map: Vec<String>,
    /// Manifest keys of the chunks that hold map-engine modules.
    map_keys: std::collections::BTreeSet<String>,
}

/// Checks the admin isolation rule on the real module lists (and, without a list, on the manifest's entry modules).
fn admin_findings(
    chunks: &std::collections::BTreeMap<String, Chunk>,
    modules: Option<&ModuleList>,
    reachable_from_index: &std::collections::BTreeSet<String>,
    admin_entry_static: &std::collections::BTreeSet<String>,
) -> AdminFindings {
    let mut out = AdminFindings::default();
    // modules of every chunk, from the list when there is one, else the one module the manifest names
    let mods_of = |chunk: &Chunk| -> Vec<String> {
        match modules.and_then(|m| m.get(&chunk.file)) {
            Some(list) => list.clone(),
            None => chunk.src.iter().cloned().collect(),
        }
    };
    for (key, chunk) in chunks {
        let mods = mods_of(chunk);
        if reachable_from_index.contains(key) {
            for m in mods.iter().filter(|m| m.starts_with(ADMIN_SOURCES)) {
                out.leaked.push(format!("{}: {m}", chunk.file));
            }
        }
        let map: Vec<&String> = mods
            .iter()
            .filter(|m| m.starts_with(ADMIN_MAP_SOURCES))
            .collect();
        if !map.is_empty() {
            out.map_keys.insert(key.clone());
            if admin_entry_static.contains(key) {
                for m in map {
                    out.eager_map.push(format!("{}: {m}", chunk.file));
                }
            }
        }
    }
    out
}

/// What the size report measured (gzip bytes), for the tests; `over` lists the size targets that were exceeded
/// (informational).
#[derive(Debug, Default)]
struct Sizes {
    /// The player's client: everything the player's page can reach, apart from the console's and the dev surfaces' own files.
    client_gz: usize,
    /// The console's own files (its entry, its lazy chunks, its css, `admin.html`).
    admin_gz: usize,
    /// The console's entry JS without the chunks the player client shares.
    admin_entry_gz: usize,
    /// The map chunk, `None` while the console has none.
    admin_map_gz: Option<usize>,
    over: Vec<String>,
}

/// The size report and the isolation checks of WP-CLIENT-17 and WP-ADMIN-03. Removes `build-manifest.json` and
/// `build-modules.json` afterwards.
fn size_report(dist: &Path) -> Result<Sizes> {
    let chunks = read_chunks(dist)?;
    let modules = read_modules(dist)?;
    let mut over: Vec<String> = Vec::new();

    // the code of the page before the login screen: the entry and its static imports, then the boot chunk
    // (`import('./app/boot')`, loaded by main.ts) and ITS static imports; dynamic imports (renderer, dev) are lazy
    let entry_keys = closure(&chunks, &["index.html"], false);
    let boot_key = chunks
        .iter()
        .find(|(_, c)| c.src.as_deref() == Some("src/app/boot.ts"))
        .map(|(k, _)| k.clone())
        .context("the build has no chunk for src/app/boot.ts (main.ts imports it lazily)")?;
    let mut path_keys = entry_keys.clone();
    path_keys.extend(closure(&chunks, &[boot_key.as_str()], false));
    let js_of = |keys: &std::collections::BTreeSet<String>| -> Result<(usize, usize)> {
        let (mut raw, mut gz) = (0, 0);
        for k in keys {
            let Some(c) = chunks.get(k) else { continue };
            if c.file.ends_with(".js") {
                raw += std::fs::metadata(dist.join(&c.file))?.len() as usize;
                gz += gz_len(dist, &c.file)?;
            }
        }
        Ok((raw, gz))
    };
    let (entry_raw, entry_gz) = js_of(&entry_keys)?;
    let (path_raw, path_gz) = js_of(&path_keys)?;

    // the lazy renderer chunk (three.js and render/)
    let lazy = chunks
        .iter()
        .find(|(_, c)| c.src.as_deref() == Some("src/render/index.ts"))
        .map(|(k, _)| closure(&chunks, &[k.as_str()], false))
        .unwrap_or_default();
    let lazy_only: std::collections::BTreeSet<String> =
        lazy.difference(&path_keys).cloned().collect();
    let (lazy_raw, lazy_gz) = js_of(&lazy_only)?;

    // who owns which output file: the admin console's and the dev surfaces' own files are apart from the player's client
    let reachable = closure(&chunks, &["index.html"], true);
    let has_admin = chunks.contains_key("admin.html");
    let admin_files = if has_admin {
        exclusive_files(&chunks, "admin.html", &["index.html", "dev.html"])
    } else {
        std::collections::BTreeSet::new()
    };
    let dev_files = if chunks.contains_key("dev.html") {
        exclusive_files(&chunks, "dev.html", &["index.html", "admin.html"])
    } else {
        std::collections::BTreeSet::new()
    };

    // wasm, fonts, workers, the player client, the admin console, the dev surfaces
    let mut wasm = (0usize, 0usize);
    let mut workers = 0usize;
    let mut client = (0usize, 0usize); // gzip bytes, files
    let mut admin = (0usize, 0usize);
    let mut dev = (0usize, 0usize);
    for entry in walkdir::WalkDir::new(dist) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".gz") || name == BUILD_MANIFEST || name == BUILD_MODULES {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(dist)?
            .to_string_lossy()
            .replace('\\', "/");
        let raw = entry.metadata()?.len() as usize;
        let gz = gz_len(dist, &rel)?.min(raw);
        if admin_files.contains(&rel) {
            admin = (admin.0 + gz, admin.1 + 1);
            continue;
        }
        if dev_files.contains(&rel) {
            dev = (dev.0 + gz, dev.1 + 1);
            continue;
        }
        client = (client.0 + gz, client.1 + 1);
        if name.ends_with(".wasm") {
            wasm = (raw, gz);
        }
        if name.contains(".worker-") {
            workers += 1;
        }
    }
    let (total_gz, files) = client;

    println!(
        "    initial JS (entry + static imports)  {:>10}  gzip  ({} raw)",
        kib(entry_gz),
        kib(entry_raw)
    );
    println!(
        "    boot path to the login screen        {:>10}  gzip  ({} raw; target {})",
        kib(path_gz),
        kib(path_raw),
        kib(BUDGET_INITIAL_JS_GZ)
    );
    println!(
        "    lazy renderer chunk (three.js)       {:>10}  gzip  ({} raw)",
        kib(lazy_gz),
        kib(lazy_raw)
    );
    println!(
        "    wasm                                 {:>10}  gzip  ({} raw; target {} / {})",
        kib(wasm.1),
        kib(wasm.0),
        kib(BUDGET_WASM_GZ),
        kib(BUDGET_WASM_RAW)
    );
    println!(
        "    player client                        {:>10}  gzip  ({files} files, {workers} worker chunk(s); target {})",
        kib(total_gz),
        kib(BUDGET_TOTAL_GZ)
    );
    if path_gz > BUDGET_INITIAL_JS_GZ {
        over.push(format!(
            "boot path {} > {}",
            kib(path_gz),
            kib(BUDGET_INITIAL_JS_GZ)
        ));
    }
    if wasm.0 > BUDGET_WASM_RAW || wasm.1 > BUDGET_WASM_GZ {
        over.push(format!("wasm {} raw / {} gzip", kib(wasm.0), kib(wasm.1)));
    }
    if total_gz > BUDGET_TOTAL_GZ {
        over.push(format!(
            "player client {} gzip > {}",
            kib(total_gz),
            kib(BUDGET_TOTAL_GZ)
        ));
    }

    // the admin console (Plan/25 §11.2, §13): isolation, laziness of the map, size targets
    let admin_entry_static: std::collections::BTreeSet<String> = if has_admin {
        closure(&chunks, &["admin.html"], false)
    } else {
        std::collections::BTreeSet::new()
    };
    let findings = admin_findings(&chunks, modules.as_ref(), &reachable, &admin_entry_static);
    let mut sizes = Sizes {
        client_gz: total_gz,
        admin_gz: admin.0,
        ..Sizes::default()
    };
    if has_admin {
        let entry_only: std::collections::BTreeSet<String> =
            admin_entry_static.difference(&reachable).cloned().collect();
        let (a_raw, a_gz) = js_of(&entry_only)?;
        sizes.admin_entry_gz = a_gz;
        println!(
            "    admin entry JS (without shared)      {:>10}  gzip  ({} raw; target {})",
            kib(a_gz),
            kib(a_raw),
            kib(BUDGET_ADMIN_ENTRY_GZ)
        );
        if a_gz > BUDGET_ADMIN_ENTRY_GZ {
            over.push(format!(
                "admin entry JS {} > {}",
                kib(a_gz),
                kib(BUDGET_ADMIN_ENTRY_GZ)
            ));
        }
        // the map chunk: the chunks that hold the map engine and what only they import, minus the entry and the shared chunks
        let mut map_closure = std::collections::BTreeSet::new();
        for k in &findings.map_keys {
            map_closure.extend(closure(&chunks, &[k.as_str()], false));
        }
        let map_only: std::collections::BTreeSet<String> = map_closure
            .difference(&admin_entry_static)
            .filter(|k| !reachable.contains(*k))
            .cloned()
            .collect();
        if findings.map_keys.is_empty() {
            println!(
                "    admin map chunk                      {:>10}  (not built yet; target {})",
                "-",
                kib(BUDGET_ADMIN_MAP_GZ)
            );
        } else {
            let (m_raw, m_gz) = js_of(&map_only)?;
            sizes.admin_map_gz = Some(m_gz);
            println!(
                "    admin map chunk                      {:>10}  gzip  ({} raw; target {})",
                kib(m_gz),
                kib(m_raw),
                kib(BUDGET_ADMIN_MAP_GZ)
            );
            if m_gz > BUDGET_ADMIN_MAP_GZ {
                over.push(format!(
                    "admin map chunk {} > {}",
                    kib(m_gz),
                    kib(BUDGET_ADMIN_MAP_GZ)
                ));
            }
        }
        println!(
            "    admin console files                  {:>10}  gzip  ({} files, apart from the player's client)",
            kib(admin.0),
            admin.1
        );
    } else {
        println!("    admin console: no admin.html entry in this build");
    }
    if dev.1 > 0 {
        println!(
            "    dev surfaces                         {:>10}  gzip  ({} files, apart from the player's client)",
            kib(dev.0),
            dev.1
        );
    }

    // isolation: nothing a player's page loads may contain a dev surface or a module of the admin console
    let leaked: Vec<String> = reachable
        .iter()
        .filter_map(|k| chunks.get(k))
        .filter_map(|c| c.src.clone())
        .filter(|src| DEV_ONLY_SOURCES.iter().any(|p| src.starts_with(p)))
        .collect();
    let dev_entry = chunks
        .get("dev.html")
        .map(|c| c.file.clone())
        .unwrap_or_default();
    if leaked.is_empty() {
        println!(
            "    dev surfaces: separate dev chunk ({dev_entry}), not reachable from index.html"
        );
    }
    if findings.leaked.is_empty() && has_admin {
        println!(
            "    admin console: separate entry ({}), no src/admin/ module reachable from index.html",
            chunks
                .get("admin.html")
                .map(|c| c.file.as_str())
                .unwrap_or_default()
        );
    }

    // the build's own bookkeeping never ships (nor its precompressed copies from step 5)
    for name in [BUILD_MANIFEST, BUILD_MODULES] {
        let _ = std::fs::remove_file(dist.join(name));
        let _ = std::fs::remove_file(dist.join(format!("{name}.gz")));
    }
    if !leaked.is_empty() {
        bail!(
            "the index.html chunk graph reaches dev-only modules: {}",
            leaked.join(", ")
        );
    }
    if !findings.leaked.is_empty() {
        bail!(
            "the index.html chunk graph reaches src/admin/ modules (Plan/25 §11.2: the console is a separate entry): {}",
            findings.leaked.join(", ")
        );
    }
    // size only (Plan/25 §3.8): reported, never a failure since 2026-10-29 (owner)
    if !findings.eager_map.is_empty() {
        println!(
            "    note: the admin map engine is inside the admin entry chunk (informational): {}",
            findings.eager_map.join(", ")
        );
    }
    // Plan/02 §8.8, Plan/25 §13: size targets, reported only since 2026-10-29 (owner)
    for o in &over {
        println!("    note: over the size target (informational): {o}");
    }
    sizes.over = over;
    Ok(sizes)
}

/// Plan/02 §4.4 step 6 and the acceptance criteria of WP-CORE-04 / WP-CLIENT-01.
fn check_dist_shape(dist: &Path) -> Result<()> {
    let mut problems = Vec::new();
    for required in ["index.html", "admin.html", "build-id.txt"] {
        if !dist.join(required).is_file() {
            problems.push(format!("missing {required}"));
        }
    }
    for entry in std::fs::read_dir(dist).with_context(|| format!("read {}", dist.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && name != "assets" {
            problems.push(format!(
                "unexpected directory {name}/ (the server serves only /assets/* and single-segment root files; \
                 put static files under web/src/static/ and import them so Vite emits hashed files in assets/)"
            ));
        }
    }
    let mut hashed_font = false;
    let assets = dist.join("assets");
    if assets.is_dir() {
        for entry in walkdir::WalkDir::new(&assets) {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".map") {
                problems.push(format!("source map shipped: {name}"));
            }
            if name.ends_with(".woff2") && name.contains('-') {
                hashed_font = true;
            }
        }
    } else {
        problems.push("missing assets/ directory".to_owned());
    }
    if !hashed_font {
        problems.push("no content-hashed .woff2 under assets/ (fonts.css import lost?)".to_owned());
    }
    match std::fs::read_to_string(dist.join("index.html")) {
        Ok(html) if html.contains("%FLYFF_TITLE%") => {}
        Ok(_) => problems.push(
            "index.html lost the %FLYFF_TITLE% placeholder (Plan/24 section 4.3 substitutes it)"
                .to_owned(),
        ),
        Err(_) => {}
    }
    if problems.is_empty() {
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  dist-shape: {p}");
        }
        bail!(
            "web/dist has the wrong shape ({} problem(s) above)",
            problems.len()
        )
    }
}
