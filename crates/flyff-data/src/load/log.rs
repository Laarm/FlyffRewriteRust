//! The `logs/data-load.jsonl` writer (Plan/14 §8.3): the full diagnostics list, one JSON object per line,
//! overwritten at each start, followed by one summary line. It is never written into `Resource/`.

use flyff_script::Diagnostics;
use std::io::{self, Write};
use std::path::Path;

/// Writes every stored diagnostic as one JSON line (`severity`, `code`, `file`, `line`, `message`), then a
/// last line `{"summary": {"counts": {"Error:UnknownIdentifier": n, ...}, "stored": n, "dropped": n}}`.
pub fn write_diagnostics_jsonl(diags: &Diagnostics, out: &mut impl Write) -> io::Result<()> {
    for d in diags.items() {
        let line = serde_json::to_string(d).map_err(io::Error::other)?;
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
    }
    let summary = diags.summary();
    let line = serde_json::json!({
        "summary": {
            "counts": summary.counts,
            "stored": diags.items().len(),
            "dropped": diags.dropped(),
        }
    });
    out.write_all(line.to_string().as_bytes())?;
    out.write_all(b"\n")
}

/// [`write_diagnostics_jsonl`] into `path` (parent directories are created, an existing file is replaced).
pub fn write_diagnostics_file(diags: &Diagnostics, path: &Path) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut buf = Vec::new();
    write_diagnostics_jsonl(diags, &mut buf)?;
    std::fs::write(path, buf)
}
