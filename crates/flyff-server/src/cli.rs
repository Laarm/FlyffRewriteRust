//! The `flyff` command line (Plan/10 §9). `flyff` without a subcommand is `flyff serve`.
//!
//! Implemented here: `serve`, `check` ([`check`]), `prepare` ([`prepare`]), `db backup|restore|check|migrate|
//! vacuum` and `character undelete` ([`db`]), `quiz import` ([`quiz`]), `config default|show`, `version`. The clap
//! tree of §9.2 is complete; the `account` and `grant` subtrees dispatch to the hooks in [`crate::wiring`] (the
//! handlers belong to docs 12 and 23), and the commands of later work packages (`replay`, `export`) print "not
//! available in this build yet" and exit 1. clap usage errors exit 64.

// CLI output is this module's job.
#![allow(clippy::print_stdout, clippy::print_stderr)]

pub mod account;
pub mod admin_client;
pub mod check;
pub mod db;
pub mod export;
pub mod prepare;
pub mod quiz;

use crate::build_info;
use crate::config::{self, CliOverrides};
use crate::exit::{self, ExitStatusCode};
use crate::runtime::{self, ServerOptions, StartupError, resolve_home};
use crate::web_dist::WebDistSource;
use crate::wiring;
use clap::{ArgGroup, Args, Parser, Subcommand};
use std::path::PathBuf;

/// `flyff` — the Flyff V15 server.
#[derive(Parser, Debug)]
#[command(
    name = "flyff",
    about = "Flyff V15 server (single executable: game server, web server, embedded browser client)"
)]
#[command(disable_version_flag = true, disable_help_subcommand = true)]
pub struct Cli {
    /// Runtime home (default: the folder of the executable). Env: FLYFF_HOME.
    #[arg(long, global = true, value_name = "DIR")]
    pub home: Option<PathBuf>,
    /// Location of the original `Resource/` folder. Env: FLYFF_RESOURCE.
    #[arg(long, global = true, value_name = "DIR")]
    pub resource: Option<PathBuf>,
    /// Serve the web client from this directory instead of the embedded build.
    #[arg(long, global = true, value_name = "DIR")]
    pub web_dir: Option<PathBuf>,
    /// Developer mode: `/dev/*` surfaces, Vite origins, console on stdin.
    #[arg(long, global = true)]
    pub dev: bool,
    /// Listen address, overrides `net.bind` (for example 127.0.0.1:8080).
    #[arg(long, global = true, value_name = "ADDR")]
    pub bind: Option<String>,
    /// Print the version line and exit.
    #[arg(short = 'V', long = "version")]
    pub version: bool,
    /// Subcommand (default: serve).
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Subcommands (Plan/10 §9.2).
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Run the server (the default).
    Serve,
    /// Build the derived assets ahead of time (cache/); may run while the server runs.
    Prepare(PrepareArgs),
    /// Validate configuration, Resource, data, caches and the database.
    Check {
        /// Compare the data counts and the binary-file tallies with the original Resource (and the cache budget) and fail on a difference.
        #[arg(long)]
        expect_reference: bool,
        /// More detail.
        #[arg(long)]
        verbose: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },
    /// Account administration (Plan/12 §5.5).
    #[command(subcommand)]
    Account(AccountCmd),
    /// Queue an item or gold grant for a character (Plan/23 §8.7).
    Grant(GrantArgs),
    /// Database maintenance.
    #[command(subcommand)]
    Db(DbCmd),
    /// Character maintenance.
    #[command(subcommand)]
    Character(CharacterCmd),
    /// Quiz questions.
    #[command(subcommand)]
    Quiz(QuizCmd),
    /// Replay a recorded run (Plan/10 §16).
    Replay(ReplayArgs),
    /// Debug exports.
    #[command(subcommand)]
    Export(ExportCmd),
    /// Configuration helpers.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Print the version line.
    Version,
}

/// `flyff prepare` (Plan/10 §9.2, 24 §11.3).
#[derive(Args, Debug, Clone, Default, PartialEq, Eq)]
pub struct PrepareArgs {
    /// Only this kind (slug, repeatable): data-bundles, motion-table, model-bounds, collision, ctrl-models,
    /// boss-rigs, png, cursor, dds, gz, pack-land, pack-skel.
    #[arg(long, value_name = "SLUG", value_parser = parse_kind_slug)]
    pub kind: Vec<String>,
    /// Only this world folder for collision and the land packs (repeatable), for example wdmadrigal.
    #[arg(long, value_name = "FOLDER")]
    pub world: Vec<String>,
    /// Worker threads (default: all cores, not niced).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=256))]
    pub threads: Option<u32>,
    /// Rebuild the selected kinds even when they are cached.
    #[arg(long)]
    pub force: bool,
    /// Skip the gzip variants of raw files.
    #[arg(long)]
    pub no_gzip: bool,
    /// Write the JSON report here (default: cache/prepare-report.json).
    #[arg(long, value_name = "FILE")]
    pub report: Option<PathBuf>,
}

fn parse_kind_slug(s: &str) -> Result<String, String> {
    if flyff_assets::AssetKind::from_slug(s).is_some() {
        Ok(s.to_owned())
    } else {
        let all: Vec<&str> = flyff_assets::AssetKind::ALL
            .iter()
            .map(|k| k.slug())
            .collect();
        Err(format!("unknown kind '{s}' (one of: {})", all.join(", ")))
    }
}

/// `flyff account ...` (Plan/12 §5.5; the flags are exactly the table there).
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum AccountCmd {
    /// Create an account.
    Create {
        /// Account name (3..16 characters, lower-case letters and digits, starting with a letter).
        name: String,
        /// Authority level (letter D..P or Z, AUTH_* token, or player/helper/gm1/gm2/gm3/operator/admin).
        #[arg(long)]
        auth: Option<String>,
        /// Read the password as one line from stdin.
        #[arg(long, conflicts_with = "random")]
        password_stdin: bool,
        /// Generate a random password and print it once.
        #[arg(long)]
        random: bool,
        /// E-mail address (optional).
        #[arg(long)]
        email: Option<String>,
    },
    /// Change the password of an account.
    Passwd {
        /// Account name.
        name: String,
        /// Read the password as one line from stdin.
        #[arg(long, conflicts_with = "random")]
        password_stdin: bool,
        /// Generate a random password and print it once.
        #[arg(long)]
        random: bool,
    },
    /// Set the authority of an account (or of one of its characters).
    SetAuth {
        /// Account name.
        name: String,
        /// New authority level.
        level: String,
        /// Set the authority of this character of the account instead.
        #[arg(long)]
        character: Option<String>,
    },
    /// List accounts.
    List {
        /// Case-insensitive substring of the account name.
        #[arg(long)]
        name: Option<String>,
        /// Only accounts with GM authority.
        #[arg(long)]
        gm: bool,
        /// Only online accounts.
        #[arg(long)]
        online: bool,
        /// At most this many rows.
        #[arg(long, default_value_t = 1000)]
        limit: u32,
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Ban an account (or lift its bans).
    #[command(group(ArgGroup::new("when").required(true).args(["for_", "until", "lift"])))]
    Ban {
        /// Account name.
        name: String,
        /// Duration: a number followed by m, h or d (for example 90m, 12h, 7d), or perm.
        #[arg(long = "for", value_name = "DUR")]
        for_: Option<String>,
        /// Until this date (YYYY-MM-DD or YYYY-MM-DDTHH:MM in server.timezone, or RFC 3339).
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
        /// Lift every active ban.
        #[arg(long, conflicts_with = "reason")]
        lift: bool,
        /// Reason shown in the audit log.
        #[arg(long)]
        reason: Option<String>,
    },
}

/// `flyff grant <character> (--item <id> [--count <n>] | --gold <n>) [--note <text>]`.
#[derive(Args, Debug, Clone, PartialEq, Eq)]
#[command(group(ArgGroup::new("what").required(true).multiple(false).args(["item", "gold"])))]
pub struct GrantArgs {
    /// Character name.
    pub character: String,
    /// Item id (define value).
    #[arg(long)]
    pub item: Option<u32>,
    /// Item count (default 1).
    #[arg(long, conflicts_with = "gold")]
    pub count: Option<u32>,
    /// Gold amount.
    #[arg(long)]
    pub gold: Option<u64>,
    /// Note stored with the grant.
    #[arg(long)]
    pub note: Option<String>,
}

/// `flyff db ...`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum DbCmd {
    /// Online backup into `backups/` (works while the server runs).
    Backup {
        /// Also copy flyff.toml.
        #[arg(long)]
        with_config: bool,
    },
    /// Restore a backup over the database (the server must be stopped).
    Restore {
        /// Backup file.
        file: PathBuf,
        /// Do not ask.
        #[arg(long)]
        yes: bool,
    },
    /// Consistency checks (read-only, safe while the server runs).
    Check {
        /// `PRAGMA quick_check`.
        #[arg(long, conflicts_with_all = ["full", "serials", "items"])]
        quick: bool,
        /// `PRAGMA integrity_check`.
        #[arg(long, conflicts_with_all = ["serials", "items"])]
        full: bool,
        /// Item serial uniqueness.
        #[arg(long, conflicts_with = "items")]
        serials: bool,
        /// Item ids against the game data.
        #[arg(long)]
        items: bool,
    },
    /// Apply (or list with --dry-run) the pending schema migrations.
    Migrate {
        /// Only list the pending migrations.
        #[arg(long)]
        dry_run: bool,
    },
    /// Backup, VACUUM and checkpoint (the server must be stopped).
    Vacuum,
}

/// `flyff character ...`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum CharacterCmd {
    /// Restore a soft-deleted character into a free slot (the server must be stopped).
    Undelete {
        /// Name or id.
        target: String,
        /// Slot to restore into.
        #[arg(long)]
        slot: Option<u8>,
    },
}

/// `flyff quiz ...`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum QuizCmd {
    /// Import quiz questions (WP-OPS-17).
    Import {
        /// File.
        file: PathBuf,
        /// Quiz type: 1 = OX, 2 = four choices. Imports only the entries of that type.
        #[arg(long = "type", value_parser = clap::value_parser!(u32).range(1..=2))]
        quiz_type: Option<u32>,
        /// Also delete the questions of `--type` that are not in the file.
        #[arg(long, requires = "quiz_type")]
        replace: bool,
    },
}

/// `flyff replay <DIR>` (WP-OPS-13).
#[derive(Args, Debug, Clone, PartialEq, Eq)]
pub struct ReplayArgs {
    /// The recorded run.
    pub dir: PathBuf,
    /// Stop at this tick.
    #[arg(long)]
    pub until_tick: Option<u64>,
    /// Verify the state digests.
    #[arg(long)]
    pub verify: bool,
    /// Dump one session's messages.
    #[arg(long)]
    pub dump_session: Option<u64>,
    /// Replay despite a build mismatch.
    #[arg(long)]
    pub force: bool,
}

/// `flyff export ...`.
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum ExportCmd {
    /// One JSON file per GameData section (WP-DATA-17).
    GamedataJson {
        /// Output directory.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// A model with animations as glTF (debug).
    ModelGltf {
        /// Model name.
        model: String,
        /// Animation clip (repeatable).
        #[arg(long)]
        motion: Vec<String>,
        /// Output file.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// `flyff config ...`.
#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCmd {
    /// Print the commented default `flyff.toml`.
    Default,
    /// Print the effective configuration with the source of every changed value.
    Show {
        /// Only values that differ from the defaults.
        #[arg(long)]
        changed: bool,
    },
}

/// What `main` runs: [`run_from_args`], after restoring the default `SIGPIPE` for every command except `serve`
/// (`flyff check | head` ends quietly instead of panicking in `println!`; a server must see `EPIPE` as an error).
pub fn run_main<I, T>(args: I) -> ExitStatusCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => {
            if !matches!(cli.command, None | Some(Command::Serve)) {
                crate::sys::reset_sigpipe();
            }
            run(cli)
        }
        Err(e) => {
            let _ = e.print();
            if e.use_stderr() {
                exit::USAGE
            } else {
                exit::OK
            }
        }
    }
}

/// Parses `args` (including `argv[0]`) and runs the command; returns the exit code.
pub fn run_from_args<I, T>(args: I) -> ExitStatusCode
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    match Cli::try_parse_from(args) {
        Ok(cli) => run(cli),
        Err(e) => {
            let _ = e.print();
            if e.use_stderr() {
                exit::USAGE
            } else {
                exit::OK
            }
        }
    }
}

/// Runs a parsed command line.
pub fn run(cli: Cli) -> ExitStatusCode {
    if cli.version {
        return cmd_version(&cli);
    }
    match cli.command.clone().unwrap_or(Command::Serve) {
        Command::Serve => cmd_serve(&cli),
        Command::Version => cmd_version(&cli),
        Command::Check {
            expect_reference,
            verbose,
            json,
        } => check::run(
            &cli,
            &check::CheckOptions {
                expect_reference,
                verbose,
                json,
            },
        ),
        Command::Prepare(args) => prepare::run(&cli, &args),
        Command::Config(c) => cmd_config(&cli, &c),
        Command::Account(a) => wiring::run_account_cli(&cli, &a),
        Command::Grant(g) => wiring::run_grant_cli(&cli, &g),
        Command::Db(DbCmd::Migrate { dry_run }) => cmd_db_migrate(&cli, dry_run),
        Command::Db(cmd) => db::run(&cli, &cmd),
        Command::Character(CharacterCmd::Undelete { target, slot }) => {
            db::undelete(&cli, &target, slot)
        }
        Command::Quiz(q) => quiz::run(&cli, &q),
        Command::Replay(_) => not_available("replay"),
        Command::Export(ExportCmd::GamedataJson { out }) => {
            export::run_gamedata_json(&cli, out.as_deref())
        }
        Command::Export(_) => not_available("export"),
    }
}

fn not_available(cmd: &str) -> ExitStatusCode {
    eprintln!("flyff {cmd}: not available in this build yet");
    exit::FAILURE
}

pub(crate) fn web_source(cli: &Cli) -> WebDistSource {
    WebDistSource::select(cli.web_dir.clone())
}

fn cmd_version(cli: &Cli) -> ExitStatusCode {
    println!("{}", build_info::version_line(&web_source(cli).build_id()));
    if let Some(line) = database_line(cli) {
        println!("{line}");
    }
    exit::OK
}

/// `database: schema N (this build: M)` when the home has a readable database (Plan/10 §9.2 `version`).
fn database_line(cli: &Cli) -> Option<String> {
    let home = resolve_home(cli.home.as_deref()).ok()?;
    let cfg = config::load(&home, &overrides_of(cli), &env_pairs(), &[])
        .ok()?
        .config;
    let path = cfg.resolve_paths(&home).database;
    if !path.is_file() {
        return None;
    }
    let conn = flyff_db::conn::open_reader(&path, &cfg.database).ok()?;
    let status = flyff_db::migrate::status(&conn).ok()?;
    Some(format!(
        "database: schema {} (this build: {})",
        status.current, status.latest
    ))
}

pub(crate) fn env_pairs() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("FLYFF_"))
        .collect()
}

/// The home directory of a command, or the exit code to return.
pub(crate) fn home_of(cli: &Cli) -> Result<PathBuf, ExitStatusCode> {
    resolve_home(cli.home.as_deref()).map_err(|e| {
        eprintln!("error: {e}");
        exit::FAILURE
    })
}

/// The flag overrides of a command (`--resource` falls back to `FLYFF_RESOURCE` through the loader's env pass).
pub(crate) fn overrides_of(cli: &Cli) -> CliOverrides {
    let mut resource = cli.resource.clone();
    if resource.is_none() {
        resource = std::env::var_os("FLYFF_RESOURCE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
    }
    CliOverrides {
        bind: cli.bind.clone(),
        resource,
        dev: cli.dev,
    }
}

fn cmd_serve(cli: &Cli) -> ExitStatusCode {
    let home = match home_of(cli) {
        Ok(h) => h,
        Err(c) => return c,
    };
    let bind = match cli
        .bind
        .as_deref()
        .map(str::parse::<std::net::SocketAddr>)
        .transpose()
    {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: --bind is not a socket address: {e}");
            return exit::USAGE;
        }
    };
    let mut opts = ServerOptions::new(home);
    opts.resource = cli.resource.clone();
    opts.bind = bind;
    opts.dev = cli.dev;
    opts.web_dir = cli.web_dir.clone();
    opts.console = true;
    opts.print_banner = true;
    // Process tests of the exit codes and SIGTERM run against a fake Resource folder; this switch exists only
    // in builds with the `test-hooks` feature (never in a shipped binary).

    match runtime::start(opts) {
        Ok(server) => {
            // the whole stop may take `countdown + 90 s`; a stuck step must not hang it forever (10 §15.2 step 6)
            let watchdog = crate::shutdown::StopWatchdog::spawn(
                server.coordinator().clone(),
                std::time::Duration::from_millis(500),
                || {
                    eprintln!("error: the shutdown did not complete in time: exiting");
                    std::process::exit(exit::FAILURE);
                },
            );
            let code = server.wait();
            watchdog.finish();
            code
        }
        // a signal during the start-up: everything opened was shut down gracefully (Plan/10 §15.2), exit 0
        Err(e @ StartupError::Interrupted(_)) => {
            eprintln!("{e}");
            e.exit_code()
        }
        Err(e) => {
            eprintln!("error: {e}");
            e.exit_code()
        }
    }
}

fn cmd_db_migrate(cli: &Cli, dry_run: bool) -> ExitStatusCode {
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
    let now = flyff_common::UnixTime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
    );
    match flyff_db::migrate::cli_migrate(&loaded.config.persistence(), &home, dry_run, now) {
        Ok(out) => {
            println!(
                "schema version {} of {}",
                out.before.current, out.before.latest
            );
            if out.before.pending.is_empty() {
                println!("no pending migrations");
            }
            for m in &out.before.pending {
                println!(
                    "{} migration {:04}_{}",
                    if dry_run { "pending:" } else { "applied:" },
                    m.version,
                    m.name
                );
            }
            if let Some(b) = out
                .report
                .as_ref()
                .and_then(|r| r.premigrate_backup.as_ref())
            {
                println!("pre-migration backup: {}", b.display());
            }
            exit::OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            match e.kind {
                flyff_db::DbErrorKind::Locked => exit::TEMPFAIL,
                flyff_db::DbErrorKind::Corrupt => exit::IOERR,
                flyff_db::DbErrorKind::InitOnlyMismatch | flyff_db::DbErrorKind::InvalidConfig => {
                    exit::CONFIG
                }
                _ => exit::FAILURE,
            }
        }
    }
}

fn cmd_config(cli: &Cli, c: &ConfigCmd) -> ExitStatusCode {
    match c {
        ConfigCmd::Default => {
            print!("{}", config::TEMPLATE);
            exit::OK
        }
        ConfigCmd::Show { changed } => {
            let home = match home_of(cli) {
                Ok(h) => h,
                Err(c) => return c,
            };
            let overrides = CliOverrides {
                bind: cli.bind.clone(),
                resource: cli.resource.clone(),
                dev: cli.dev,
            };
            match config::load(&home, &overrides, &env_pairs(), &[]) {
                Ok(loaded) => {
                    for w in &loaded.warnings {
                        eprintln!("warning: {w}");
                    }
                    print!("{}", config::render_show(&loaded, *changed));
                    exit::OK
                }
                Err(errs) => {
                    for e in errs {
                        eprintln!("error: {e}");
                    }
                    exit::CONFIG
                }
            }
        }
    }
}

/// Used by `main` to print start-up errors uniformly.
pub fn report_startup_error(e: &StartupError) -> ExitStatusCode {
    eprintln!("error: {e}");
    e.exit_code()
}
