//! `flyff prepare` (Plan/10 §9.2, 24 §11.3): builds the derived assets ahead of the first start.
//!
//! No database, no instance lock and no game thread: config, VFS, `GameData`, `AssetPrep` with `--threads`
//! un-niced workers (default all cores), the stale sweep, every job of the selected kinds
//! ([`flyff_assets::AssetPrep::prepare_all`]), the JSON report (`cache/prepare-report.json` or `--report`) and
//! a per-kind summary with the budget usage. A full prepare (no `--kind`, no `--world`) then runs the
//! server-side load of the start-up kinds once (`load_server_derived`), which also publishes the `motions` and
//! `model-bounds` bundle files, so the first `serve` start is a warm one.
//!
//! Exit codes: 0 = no failures, 1 = at least one builder failed (listed), 2 = fatal (config, VFS, data, I/O),
//! 66 = `Resource/` missing, 78 = invalid configuration.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::{Cli, PrepareArgs, env_pairs, home_of, overrides_of};
use crate::assets::{DataBundleBuilder, ServerClock, prep_config, start_prep};
use crate::config;
use crate::data_load::{self, DataLoadError, DataMode, LoadedData};
use crate::exit::{self, ExitStatusCode};
use crate::runtime::resource_present;
use flyff_assets::{AssetBuilder, AssetKind, AssetTelemetry, PrepareOptions, PrepareProgress};
use std::io::{IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

/// Runs `flyff prepare`.
pub fn run(cli: &Cli, args: &PrepareArgs) -> ExitStatusCode {
    init_cli_logging();
    let home = match home_of(cli) {
        Ok(h) => h,
        Err(c) => return c,
    };
    let loaded = match config::load(&home, &overrides_of(cli), &env_pairs(), &[]) {
        Ok(l) => l,
        Err(errs) => {
            for e in errs {
                eprintln!("error: {e}");
            }
            return exit::CONFIG;
        }
    };
    for w in &loaded.warnings {
        eprintln!("warning: {w}");
    }
    let cfg = loaded.config;
    let paths = cfg.resolve_paths(&home);
    if !resource_present(&paths.resource) {
        eprintln!(
            "error: the Flyff V15 Resource folder was not found at {}\n\
             Copy your original `Resource` folder next to the executable, or pass --resource <DIR>.",
            paths.resource.display()
        );
        return exit::NOINPUT;
    }
    if let Err(e) = std::fs::create_dir_all(&paths.cache) {
        eprintln!("error: cannot create {}: {e}", paths.cache.display());
        return exit::DATA;
    }
    let t0 = Instant::now();
    let LoadedData {
        vfs,
        game,
        client,
        report: data_report,
    } = match data_load::load(&cfg, &paths, DataMode::Load, None) {
        Ok(d) => d,
        Err(DataLoadError::ResourceMissing(p)) => {
            eprintln!("error: Resource folder missing: {}", p.display());
            return exit::NOINPUT;
        }
        Err(DataLoadError::Data(m)) => {
            eprintln!("error: game data: {m}");
            return exit::DATA;
        }
    };
    println!(
        "data: {} files indexed in {} ms, game data loaded in {} ms",
        data_report.vfs_files, data_report.vfs_ms, data_report.load_ms
    );

    let threads = args.threads.map_or_else(
        || std::thread::available_parallelism().map_or(1, usize::from),
        |n| n as usize,
    );
    let mut acfg = prep_config(&cfg, &paths);
    acfg.threads = threads;
    acfg.nice = 0;
    acfg.startup_threads = threads;
    acfg.prewarm = false;
    let t_start = Instant::now();
    let prep = match start_prep(
        Arc::clone(&vfs),
        Arc::clone(&game),
        acfg,
        Arc::new(ServerClock::default()),
        AssetTelemetry::new(),
        |_parts| {
            client
                .iter()
                .map(|c| Arc::new(DataBundleBuilder::new(Arc::clone(c))) as Arc<dyn AssetBuilder>)
                .collect()
        },
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: asset framework: {e}");
            return exit::DATA;
        }
    };
    println!(
        "assets: framework and data bundles ready in {} ms ({} threads)",
        t_start.elapsed().as_millis(),
        threads
    );

    let opts = PrepareOptions {
        kinds: args
            .kind
            .iter()
            .filter_map(|s| AssetKind::from_slug(s))
            .collect(),
        worlds: args.world.clone(),
        threads: 0,
        force: args.force,
        gzip: !args.no_gzip,
        report_path: args.report.clone(),
    };
    let tty = std::io::stdout().is_terminal();
    let last_pct = AtomicU8::new(255);
    let progress = |p: &PrepareProgress| {
        if p.total == 0 {
            return;
        }
        if tty {
            print!(
                "\r\x1b[2K[{:>3}%] {:<12} {}/{} {}",
                p.percent,
                p.kind,
                p.done,
                p.total,
                truncate(&p.current, 60)
            );
            let _ = std::io::stdout().flush();
        } else {
            let step = p.percent / 5 * 5;
            if p.percent == 100 || last_pct.swap(step, Ordering::Relaxed) != step {
                println!("[{:>3}%] {}/{} {}", p.percent, p.done, p.total, p.kind);
            }
        }
    };
    let report = prep.prepare_all(&opts, &progress);
    if tty {
        println!();
    }

    let mut code = print_report(&report, args);
    if args.kind.is_empty() && args.world.is_empty() && !report.has_failures() {
        let t = Instant::now();
        match prep.load_server_derived() {
            Ok(d) => println!(
                "server-side load check: ok in {} ms (motions {}, bounds {}, collision {})",
                t.elapsed().as_millis(),
                yes(d.motions.is_some()),
                yes(d.bounds.is_some()),
                yes(d.collision.is_some())
            ),
            Err(e) => {
                eprintln!("error: the server-side load of the prepared assets failed: {e}");
                code = exit::FAILURE;
            }
        }
    }
    prep.shutdown(std::time::Duration::from_secs(10));
    println!("prepare: finished in {:.1} s", t0.elapsed().as_secs_f64());
    code
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let tail: String = s
            .chars()
            .rev()
            .take(n - 3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{tail}")
    }
}

/// Prints the per-kind summary, the failures and the budget; returns 0 or 1.
fn print_report(report: &flyff_assets::PrepareReport, args: &PrepareArgs) -> ExitStatusCode {
    if report.total_jobs() == 0
        && !report.disabled.is_empty()
        && args.kind.len() == report.disabled.len()
    {
        println!(
            "nothing to prepare: {} not registered in this build",
            report.disabled.join(", ")
        );
        return exit::OK;
    }
    println!(
        "{:<14} {:>7} {:>7} {:>7} {:>7} {:>10} {:>8}",
        "kind", "jobs", "built", "cached", "failed", "MiB", "ms"
    );
    for (kind, k) in &report.kinds {
        println!(
            "{:<14} {:>7} {:>7} {:>7} {:>7} {:>10.1} {:>8}",
            kind,
            k.jobs,
            k.built,
            k.cached,
            k.failed,
            k.bytes as f64 / 1_048_576.0,
            k.ms
        );
    }
    for d in &report.disabled {
        println!("{d}: not registered in this build");
    }
    for f in &report.failures {
        eprintln!("FAILED {} {}: {}", f.kind, f.key, f.error);
    }
    let used = report.budget.used_bytes as f64 / 1_048_576.0;
    let limit = report.budget.limit_bytes as f64 / 1_048_576.0;
    println!(
        "cache: {used:.1} MiB of {limit:.0} MiB budget{}",
        if report.budget.used_bytes > report.budget.limit_bytes {
            " (OVER BUDGET: raise assets.disk_budget_mb or use assets.gzip_raw = \"lazy\")"
        } else {
            ""
        }
    );
    let path = args.report.as_ref().map_or_else(
        || "cache/prepare-report.json".to_owned(),
        |p| p.display().to_string(),
    );
    println!("report: {path}");
    report.exit_code()
}

/// `warn` and above to stderr for the CLI commands (Plan/10 §9.1), unless `RUST_LOG` says otherwise.
pub(super) fn init_cli_logging() {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_owned());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .try_init();
}
