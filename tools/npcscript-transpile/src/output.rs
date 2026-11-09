//! The output directory (Plan/20 §5.4): the `*.lua` files, `MANIFEST.txt` and `_transpile.toml`, generated
//! in memory, written, or compared byte for byte (`--check`).

use crate::emit::{FORMAT, TOOL_VERSION, emit_script};
use crate::inputs::{Inputs, sha256_hex};
use crate::model::Model;
use crate::report::Report;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

/// The name of the reproducibility record.
pub const TRANSPILE_TOML: &str = "_transpile.toml";
/// The name of the file list with checksums (`flyff check` compares `scripts/npc` against it).
pub const MANIFEST: &str = "MANIFEST.txt";

/// The generated directory content: file name to text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// Every generated file.
    pub files: BTreeMap<String, String>,
}

impl Output {
    /// The `.lua` files.
    pub fn lua_files(&self) -> impl Iterator<Item = (&String, &String)> {
        self.files.iter().filter(|(n, _)| n.ends_with(".lua"))
    }
}

/// Generates the whole directory content for a model without errors.
pub fn generate(inputs: &Inputs, model: &Model, report: &Report) -> Output {
    let mut files = BTreeMap::new();
    for (name, script) in &model.scripts {
        files.insert(format!("{name}.lua"), emit_script(model, script, inputs));
    }
    let manifest = manifest_text(&files);
    files.insert(MANIFEST.to_owned(), manifest);
    files.insert(TRANSPILE_TOML.to_owned(), transpile_toml(inputs, report));
    Output { files }
}

fn manifest_text(files: &BTreeMap<String, String>) -> String {
    let mut s = format!(
        "# content/npc: <sha256>  <file>, written by npcscript-transpile format {FORMAT}\n"
    );
    for (name, text) in files {
        let _ = writeln!(s, "{}  {name}", sha256_hex(text.as_bytes()));
    }
    s
}

fn transpile_toml(inputs: &Inputs, report: &Report) -> String {
    let c = &report.counts;
    let mut s = String::new();
    s.push_str("# Reproducibility record of content/npc, written by npcscript-transpile (Plan/20 sec. 5.4). Do not edit.\n");
    let _ = writeln!(s, "format = {FORMAT}");
    let _ = writeln!(s, "tool_version = \"{TOOL_VERSION}\"");
    s.push_str("\n[sources]\n");
    let mut sources: Vec<(&str, &str)> = vec![
        (&inputs.npc_script_cpp.name, &inputs.npc_script_cpp.sha256),
        (&inputs.helper_cpp.name, &inputs.helper_cpp.sha256),
    ];
    if let Some(f) = &inputs.npc_script_h {
        sources.push((&f.name, &f.sha256));
    }
    if let Some(f) = &inputs.functions_info_h {
        sources.push((&f.name, &f.sha256));
    }
    sources.push(("WorldDialog.txt", &inputs.dialog_sha256));
    for (name, sha) in &inputs.npc_files {
        sources.push((name, sha));
    }
    for (name, sha) in sources {
        let _ = writeln!(s, "\"{name}\" = \"{sha}\"");
    }
    s.push_str("\n[counts]\n");
    for (k, v) in [
        ("scripts", c.scripts),
        ("functions", c.functions),
        ("registrations", c.registrations),
        ("unregistered_functions", c.unregistered_functions),
        ("registration_name_differs", c.registration_name_differs),
        ("capitalised_registrations", c.capitalised_registrations),
        ("reachable", c.reachable),
        ("unreachable", c.unreachable),
        ("empty_bodies", c.empty_bodies),
        ("shapes", c.shapes),
        ("ifs", c.ifs),
        ("elses", c.elses),
        ("switches", c.switches),
        ("cases", c.cases),
        ("breaks", c.breaks),
        ("max_brace_depth", c.max_brace_depth),
        ("float_literals", c.float_literals),
        ("constants_distinct", c.constants_distinct),
        ("constants_defined", c.constants_defined),
        ("missing_string_refs", c.missing_string_refs),
        ("helpers_distinct", c.helpers_distinct),
    ] {
        let _ = writeln!(s, "{k} = {v}");
    }
    s.push_str("\n[counts.operators]\n");
    for (k, v) in &c.operators {
        let _ = writeln!(s, "\"{k}\" = {v}");
    }
    s.push_str("\n[counts.helper_calls]\n");
    for (k, v) in &c.helper_calls {
        let _ = writeln!(s, "{k} = {v}");
    }
    s.push_str("\n[counts.key_suffix_functions]\n");
    for (k, v) in &c.key_suffix_functions {
        let _ = writeln!(s, "\"{k}\" = {v}");
    }
    let _ = write!(
        s,
        "\n[shapes]\ndistinct = {}\nhistogram_sha256 = \"{}\"\n",
        c.shapes, report.shapes_sha256
    );
    s
}

/// Writes `out` into `dir`: the generated files, and removes `*.lua` files that are no longer generated.
/// Other files of the directory (`README.md`) are left alone.
pub fn write_output(dir: &Path, out: &Output) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.ends_with(".lua") && !out.files.contains_key(&name) {
            std::fs::remove_file(&path)?;
        }
    }
    for (name, text) in &out.files {
        let path = dir.join(name);
        let same = std::fs::read(&path).is_ok_and(|old| old == text.as_bytes());
        if !same {
            std::fs::write(&path, text.as_bytes())?;
        }
    }
    Ok(())
}

/// Compares `dir` with `out` byte for byte; the result lists the differences (empty = identical).
pub fn check_output(dir: &Path, out: &Output) -> Vec<String> {
    let mut diffs = Vec::new();
    for (name, text) in &out.files {
        match std::fs::read(dir.join(name)) {
            Ok(old) if old == text.as_bytes() => {}
            Ok(_) => diffs.push(format!("{name}: differs")),
            Err(_) => diffs.push(format!("{name}: missing")),
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut extra: Vec<String> = rd
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| {
                (n.ends_with(".lua") || n == MANIFEST || n == TRANSPILE_TOML)
                    && !out.files.contains_key(n)
            })
            .collect();
        extra.sort();
        diffs.extend(
            extra
                .into_iter()
                .map(|n| format!("{n}: not generated (stale)")),
        );
    }
    diffs
}
