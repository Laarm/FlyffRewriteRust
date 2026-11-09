//! `flyff export gamedata-json [--out DIR]` (Plan/14 §9, WP-DATA-17): the loaded game data as one pretty JSON
//! file per `GameData` section (`<out>/<section>.json`, default `<home>/export/gamedata`). The files are written
//! by the pure `flyff_data::check::export::export_sections`; two runs over the same data are byte-identical.
//!
//! Exit codes: 0 written, 2 the game data could not be loaded, 66 `Resource/` missing, 74 an I/O error,
//! 78 invalid configuration.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use super::{Cli, env_pairs, home_of, overrides_of, prepare::init_cli_logging};
use crate::config;
use crate::data_load::{self, DataLoadError, DataMode};
use crate::exit::{self, ExitStatusCode};
use flyff_data::check::export::export_sections;
use std::path::{Path, PathBuf};

/// Writes `sections` into `dir` (created when missing); returns the number of bytes written.
pub fn write_sections(
    dir: &Path,
    sections: &[flyff_data::check::export::ExportSection],
) -> std::io::Result<u64> {
    std::fs::create_dir_all(dir)?;
    let mut total = 0_u64;
    for s in sections {
        std::fs::write(dir.join(format!("{}.json", s.name)), &s.json)?;
        total += s.json.len() as u64;
    }
    Ok(total)
}

/// Runs `flyff export gamedata-json`.
pub fn run_gamedata_json(cli: &Cli, out: Option<&Path>) -> ExitStatusCode {
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
    let cfg = &loaded.config;
    let paths = cfg.resolve_paths(&home);
    let data = match data_load::load(cfg, &paths, DataMode::Load, None) {
        Ok(d) => d,
        Err(DataLoadError::ResourceMissing(p)) => {
            eprintln!("error: the Resource folder is missing: {}", p.display());
            return exit::NOINPUT;
        }
        Err(DataLoadError::Data(m)) => {
            eprintln!("error: the game data could not be loaded: {m}");
            return exit::DATA;
        }
    };
    let sections = match export_sections(&data.game) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return exit::SOFTWARE;
        }
    };
    let dir: PathBuf = out.map_or_else(|| home.join("export").join("gamedata"), Path::to_path_buf);
    match write_sections(&dir, &sections) {
        Ok(bytes) => {
            println!(
                "wrote {} sections ({} KiB) to {}",
                sections.len(),
                bytes / 1024,
                dir.display()
            );
            exit::OK
        }
        Err(e) => {
            eprintln!("error: cannot write {}: {e}", dir.display());
            exit::IOERR
        }
    }
}
