//! `file:line` of a message in the `.proto` sources, for the findings. The committed descriptor set carries no source
//! info (`buf build --exclude-source-info`), so the sources are scanned: a message is the line `message Name {`.

use crate::model::Model;
use std::path::{Path, PathBuf};

/// The `.proto` files of the wire package, in memory.
#[derive(Default)]
pub struct Locator {
    files: Vec<(String, Vec<String>)>,
}

impl Locator {
    /// Reads every `*.proto` under `dir/flyff/v1`; paths are reported relative to `root`.
    pub fn load(root: &Path, dir: &Path) -> Locator {
        let mut files = Vec::new();
        let v1: PathBuf = dir.join("flyff").join("v1");
        if let Ok(rd) = std::fs::read_dir(&v1) {
            let mut paths: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().is_some_and(|e| e == "proto")
                    && let Ok(text) = std::fs::read_to_string(&p)
                {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.push((rel, text.lines().map(str::to_owned).collect()));
                }
            }
        }
        Locator { files }
    }

    fn find_message(lines: &[String], name: &str, from: usize) -> Option<usize> {
        let needle = format!("message {name} ");
        let needle2 = format!("message {name}{{");
        lines
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, l)| {
                let t = l.trim_start();
                t.starts_with(&needle) || t.starts_with(&needle2)
            })
            .map(|(i, _)| i)
    }

    /// `proto/flyff/v1/x.proto:LINE` of the message `Outer.Inner` (short name, no package).
    pub fn message(&self, file_stem: &str, name: &str) -> Option<String> {
        let suffix = format!("/{file_stem}.proto");
        let (path, lines) = self.files.iter().find(|(p, _)| p.ends_with(&suffix))?;
        let mut at = 0;
        let mut found = None;
        for part in name.split('.') {
            let i = Self::find_message(lines, part, at)?;
            at = i + 1;
            found = Some(i);
        }
        found.map(|i| format!("{path}:{}", i + 1))
    }
}

/// Finds the message a finding is about from its subject text (`Name`, `Name.field`, `C2S Name tag 3`, ...).
pub fn guess_message<'a>(model: &'a Model, subject: &str) -> Option<&'a crate::model::Message> {
    for token in subject.split_whitespace().take(3) {
        if let Some(m) = model.messages.get(&format!("flyff.v1.{token}")) {
            return Some(m);
        }
        if let Some((parent, _)) = token.rsplit_once('.')
            && let Some(m) = model.messages.get(&format!("flyff.v1.{parent}"))
        {
            return Some(m);
        }
    }
    None
}
