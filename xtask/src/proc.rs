//! Child-process runner and repository context (Plan/02 §4.1).
//!
//! Rules: arguments are always passed as separate vector items (never through a shell string; the
//! reference machine's `FLYFF_RESOURCE` contains a space), `<repo>/.tools/bin` is prepended to `PATH`,
//! every step prints a numbered banner and the exact command line.

// xtask is a CLI: printing is its output channel.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where the repository is and which environment child processes get.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// Repository root (the directory holding the workspace `Cargo.toml`).
    pub root: PathBuf,
}

impl Ctx {
    /// Finds the repository root: `CARGO_MANIFEST_DIR/..` when run through cargo, else by walking up
    /// from the current directory to a `Cargo.toml` with a `[workspace]` table.
    pub fn discover() -> Result<Ctx> {
        if let Some(dir) = option_env!("CARGO_MANIFEST_DIR")
            && let Some(parent) = Path::new(dir).parent()
            && is_workspace_root(parent)
        {
            return Ok(Ctx {
                root: parent.to_path_buf(),
            });
        }
        let mut dir = std::env::current_dir().context("current directory")?;
        loop {
            if is_workspace_root(&dir) {
                return Ok(Ctx { root: dir });
            }
            if !dir.pop() {
                bail!("not inside the flyff workspace (no Cargo.toml with [workspace] found)");
            }
        }
    }

    /// `<repo>/.tools/bin`.
    pub fn tools_bin(&self) -> PathBuf {
        self.root.join(".tools").join("bin")
    }

    /// `PATH` with `.tools/bin` first.
    pub fn path_env(&self) -> std::ffi::OsString {
        let mut paths = vec![self.tools_bin()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        std::env::join_paths(paths).unwrap_or_default()
    }

    /// Joins a repository-relative path.
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

fn is_workspace_root(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("Cargo.toml")).is_ok_and(|t| t.contains("[workspace]"))
}

/// Prints `==> [name n/total] step`.
pub fn banner(name: &str, n: usize, total: usize, step: &str) {
    println!("==> [{name} {n}/{total}] {step}");
}

fn describe(program: &str, args: &[&OsStr]) -> String {
    let mut s = program.to_owned();
    for a in args {
        s.push(' ');
        let t = a.to_string_lossy();
        if t.contains(' ') {
            s.push_str(&format!("{t:?}"))
        } else {
            s.push_str(&t)
        }
    }
    s
}

/// A command under construction.
pub struct Run<'a> {
    ctx: &'a Ctx,
    program: String,
    args: Vec<std::ffi::OsString>,
    envs: Vec<(String, std::ffi::OsString)>,
    cwd: Option<PathBuf>,
}

impl<'a> Run<'a> {
    /// New command (`program` is looked up in `.tools/bin` then `PATH`).
    pub fn new(ctx: &'a Ctx, program: &str) -> Run<'a> {
        Run {
            ctx,
            program: program.to_owned(),
            args: Vec::new(),
            envs: Vec::new(),
            cwd: None,
        }
    }

    /// Adds one argument.
    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Self {
        self.args.push(a.as_ref().to_owned());
        self
    }

    /// Adds several arguments.
    pub fn args<I, S>(mut self, it: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(it.into_iter().map(|s| s.as_ref().to_owned()));
        self
    }

    /// Sets an environment variable for the child.
    pub fn env(mut self, k: &str, v: impl AsRef<OsStr>) -> Self {
        self.envs.push((k.to_owned(), v.as_ref().to_owned()));
        self
    }

    /// Sets the working directory (default: the repository root).
    #[allow(dead_code)] // API for the build-web / build-wasm commands (boot-web)
    pub fn cwd(mut self, p: impl AsRef<Path>) -> Self {
        self.cwd = Some(p.as_ref().to_path_buf());
        self
    }

    fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args);
        c.env("PATH", self.ctx.path_env());
        for (k, v) in &self.envs {
            c.env(k, v);
        }
        c.current_dir(self.cwd.as_deref().unwrap_or(&self.ctx.root));
        c
    }

    fn line(&self) -> String {
        let refs: Vec<&OsStr> = self
            .args
            .iter()
            .map(std::ffi::OsString::as_os_str)
            .collect();
        describe(&self.program, &refs)
    }

    /// Runs with inherited stdio; fails on a non-zero exit.
    pub fn run(self) -> Result<()> {
        println!("$ {}", self.line());
        let status = self
            .command()
            .status()
            .with_context(|| format!("cannot start `{}`", self.program))?;
        if !status.success() {
            bail!("`{}` failed with {status}", self.line());
        }
        Ok(())
    }

    /// Runs and returns the exit status instead of failing (stdio inherited).
    #[allow(dead_code)] // API for the build-web / build-wasm commands (boot-web)
    pub fn status(self) -> Result<std::process::ExitStatus> {
        println!("$ {}", self.line());
        self.command()
            .status()
            .with_context(|| format!("cannot start `{}`", self.program))
    }

    /// Runs quietly and returns stdout as text; fails on a non-zero exit.
    pub fn output(self) -> Result<String> {
        let out = self
            .command()
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot start `{}`", self.program))?;
        if !out.status.success() {
            bail!(
                "`{}` failed with {}: {}",
                self.line(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Runs with inherited stderr (live progress) and returns stdout lines; fails on a non-zero exit.
    /// Used for `cargo build --message-format=json-render-diagnostics`.
    pub fn stream_stdout(self) -> Result<Vec<String>> {
        println!("$ {}", self.line());
        let out = self
            .command()
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped())
            .output()
            .with_context(|| format!("cannot start `{}`", self.program))?;
        if !out.status.success() {
            bail!("`{}` failed with {}", self.line(), out.status);
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    /// Runs quietly and returns `(success, stdout, stderr)` without failing.
    pub fn capture(self) -> Result<(bool, String, String)> {
        let out = self
            .command()
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("cannot start `{}`", self.program))?;
        Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }
}
