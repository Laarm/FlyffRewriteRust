//! The two committed CSV inputs of the gate (`proto/legacy/legacy_index.csv`, `ui_send_sites.csv`; Plan/11 section 11,
//! Plan/02 section 9.6) and a small RFC 4180 reader / writer for them.

use anyhow::{Result, bail};
use std::fmt;

/// Legacy id namespace (`PT` = `PACKETTYPE_*`, `ST` = `SNAPSHOTTYPE_*`; the two overlap numerically).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ns {
    Packet,
    Snapshot,
}

impl Ns {
    /// The CSV spelling.
    pub fn tag(self) -> &'static str {
        match self {
            Ns::Packet => "PT",
            Ns::Snapshot => "ST",
        }
    }

    fn parse(s: &str) -> Result<Ns> {
        match s {
            "PT" => Ok(Ns::Packet),
            "ST" => Ok(Ns::Snapshot),
            other => bail!("unknown namespace `{other}` (want PT or ST)"),
        }
    }

    /// Number of hex digits of an id of this namespace (u32 / u16).
    pub fn width(self) -> usize {
        match self {
            Ns::Packet => 8,
            Ns::Snapshot => 4,
        }
    }
}

/// Direction of a legacy id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dir {
    C2S,
    S2C,
    /// Ids of annex section 7 that have no client endpoint.
    None,
}

impl Dir {
    /// The CSV spelling.
    pub fn tag(self) -> &'static str {
        match self {
            Dir::C2S => "C2S",
            Dir::S2C => "S2C",
            Dir::None => "-",
        }
    }

    fn parse(s: &str) -> Result<Dir> {
        match s {
            "C2S" => Ok(Dir::C2S),
            "S2C" => Ok(Dir::S2C),
            "-" => Ok(Dir::None),
            other => bail!("unknown direction `{other}`"),
        }
    }
}

/// Fate of a legacy id (annex 11a legend).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// 1:1 message with the legacy field order.
    Port,
    /// Several legacy ids map to one message (or an HTTP endpoint).
    Merge,
    /// One legacy id becomes several messages.
    Split,
    /// No message; the note gives the reason.
    Drop,
    /// Server-to-server id of annex section 7: an in-process call in the single executable (D2).
    Internal,
}

impl Status {
    /// The CSV spelling.
    pub fn tag(self) -> &'static str {
        match self {
            Status::Port => "port",
            Status::Merge => "merge",
            Status::Split => "split",
            Status::Drop => "drop",
            Status::Internal => "internal",
        }
    }

    fn parse(s: &str) -> Result<Status> {
        match s {
            "port" => Ok(Status::Port),
            "merge" => Ok(Status::Merge),
            "split" => Ok(Status::Split),
            "drop" => Ok(Status::Drop),
            "internal" => Ok(Status::Internal),
            other => bail!("unknown status `{other}`"),
        }
    }

    /// True for the statuses that must be carried by a message (or an HTTP endpoint).
    pub fn is_mapped(self) -> bool {
        matches!(self, Status::Port | Status::Merge | Status::Split)
    }
}

/// The `file` cell of a row handled by an HTTP endpoint instead of a proto message.
pub const HTTP_FILE: &str = "HTTP";

/// One row of `legacy_index.csv` (columns `ns,id,name,dir,message,file,ms,doc,status,note`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyRow {
    pub ns: Ns,
    pub id: u32,
    /// Define name without the `PACKETTYPE_` / `SNAPSHOTTYPE_` prefix, original spelling.
    pub name: String,
    pub dir: Dir,
    /// Message names in package `flyff.v1` (several = split), or `POST /api/login` for an HTTP row; empty for drops.
    pub messages: Vec<String>,
    /// `session.proto`, ..., or [`HTTP_FILE`]; empty for drops.
    pub file: String,
    pub ms: String,
    pub doc: String,
    pub status: Status,
    /// Reason of a drop, remark of the others.
    pub note: String,
}

impl LegacyRow {
    /// Hex id in the width of its namespace (`0000000B`, `00C1`).
    pub fn id_hex(&self) -> String {
        format!("{:0width$X}", self.id, width = self.ns.width())
    }

    /// True when the row is handled by an HTTP endpoint.
    pub fn is_http(&self) -> bool {
        self.file == HTTP_FILE
    }
}

impl fmt::Display for LegacyRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} ({})",
            self.ns.tag(),
            self.id_hex(),
            self.name,
            self.dir.tag()
        )
    }
}

/// Header of `legacy_index.csv`.
pub const INDEX_HEADER: &str = "ns,id,name,dir,message,file,ms,doc,status,note";
/// Header of `ui_send_sites.csv`.
pub const SITES_HEADER: &str = "send_fn,legacy_ids,call_sites,milestone,status";

/// Status of a UI send function (`ui_send_sites.csv`): `todo` until a `// legacy-send:` marker exists (rule C9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteStatus {
    Todo,
    Done,
    Dropped,
}

impl SiteStatus {
    /// The CSV spelling.
    pub fn tag(self) -> &'static str {
        match self {
            SiteStatus::Todo => "todo",
            SiteStatus::Done => "done",
            SiteStatus::Dropped => "dropped",
        }
    }

    fn parse(s: &str) -> Result<SiteStatus> {
        match s {
            "todo" => Ok(SiteStatus::Todo),
            "done" => Ok(SiteStatus::Done),
            "dropped" => Ok(SiteStatus::Dropped),
            other => bail!("unknown send-site status `{other}`"),
        }
    }
}

/// One row of `ui_send_sites.csv`: a client send function of ref/99 appendix B.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSite {
    pub send_fn: String,
    /// `(PACKETTYPE name, id)` pairs the function sends.
    pub legacy_ids: Vec<(String, u32)>,
    /// `file:line` list as printed by the appendix (at most five, `(+N)` for more).
    pub call_sites: String,
    /// Earliest milestone of the messages that replace its packets; empty when all are dropped.
    pub milestone: String,
    pub status: SiteStatus,
}

/// Splits CSV text (RFC 4180: `"` quoting, doubled quotes, embedded commas and newlines) into records.
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                other => field.push(other),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            other => field.push(other),
        }
    }
    if quoted {
        bail!("unterminated quoted field");
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// Appends one CSV record (quoting only where needed) and a newline.
pub fn write_record(out: &mut String, cells: &[&str]) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        if cell.contains([',', '"', '\n', '\r']) {
            out.push('"');
            out.push_str(&cell.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(cell);
        }
    }
    out.push('\n');
}

/// Reads `legacy_index.csv`.
pub fn read_index(text: &str) -> Result<Vec<LegacyRow>> {
    let rows = parse_csv(text)?;
    let Some((header, body)) = rows.split_first() else {
        bail!("legacy_index.csv is empty");
    };
    if header.join(",") != INDEX_HEADER {
        bail!(
            "legacy_index.csv header is `{}`, want `{INDEX_HEADER}`",
            header.join(",")
        );
    }
    let mut out = Vec::with_capacity(body.len());
    for (i, r) in body.iter().enumerate() {
        let line = i + 2;
        if r.len() != 10 {
            bail!("legacy_index.csv:{line}: {} cells, want 10", r.len());
        }
        let ns = Ns::parse(&r[0]).map_err(|e| anyhow::anyhow!("legacy_index.csv:{line}: {e}"))?;
        let id = u32::from_str_radix(&r[1], 16)
            .map_err(|e| anyhow::anyhow!("legacy_index.csv:{line}: id `{}`: {e}", r[1]))?;
        out.push(LegacyRow {
            ns,
            id,
            name: r[2].clone(),
            dir: Dir::parse(&r[3]).map_err(|e| anyhow::anyhow!("legacy_index.csv:{line}: {e}"))?,
            messages: if r[4].is_empty() {
                Vec::new()
            } else {
                r[4].split('+').map(str::to_owned).collect()
            },
            file: r[5].clone(),
            ms: r[6].clone(),
            doc: r[7].clone(),
            status: Status::parse(&r[8])
                .map_err(|e| anyhow::anyhow!("legacy_index.csv:{line}: {e}"))?,
            note: r[9].clone(),
        });
    }
    Ok(out)
}

/// Writes `legacy_index.csv` (the order of the rows is kept).
pub fn write_index(rows: &[LegacyRow]) -> String {
    let mut out = String::new();
    out.push_str(INDEX_HEADER);
    out.push('\n');
    for r in rows {
        write_record(
            &mut out,
            &[
                r.ns.tag(),
                &r.id_hex(),
                &r.name,
                r.dir.tag(),
                &r.messages.join("+"),
                &r.file,
                &r.ms,
                &r.doc,
                r.status.tag(),
                &r.note,
            ],
        );
    }
    out
}

/// Reads `ui_send_sites.csv`.
pub fn read_sites(text: &str) -> Result<Vec<UiSite>> {
    let rows = parse_csv(text)?;
    let Some((header, body)) = rows.split_first() else {
        bail!("ui_send_sites.csv is empty");
    };
    if header.join(",") != SITES_HEADER {
        bail!(
            "ui_send_sites.csv header is `{}`, want `{SITES_HEADER}`",
            header.join(",")
        );
    }
    let mut out = Vec::with_capacity(body.len());
    for (i, r) in body.iter().enumerate() {
        let line = i + 2;
        if r.len() != 5 {
            bail!("ui_send_sites.csv:{line}: {} cells, want 5", r.len());
        }
        let mut ids = Vec::new();
        for part in r[1].split(';').filter(|p| !p.is_empty()) {
            let Some((name, hex)) = part.split_once(':') else {
                bail!("ui_send_sites.csv:{line}: `{part}` is not NAME:HEX");
            };
            let id = u32::from_str_radix(hex, 16)
                .map_err(|e| anyhow::anyhow!("ui_send_sites.csv:{line}: id `{hex}`: {e}"))?;
            ids.push((name.to_owned(), id));
        }
        out.push(UiSite {
            send_fn: r[0].clone(),
            legacy_ids: ids,
            call_sites: r[2].clone(),
            milestone: r[3].clone(),
            status: SiteStatus::parse(&r[4])
                .map_err(|e| anyhow::anyhow!("ui_send_sites.csv:{line}: {e}"))?,
        });
    }
    Ok(out)
}

/// Writes `ui_send_sites.csv`.
pub fn write_sites(rows: &[UiSite]) -> String {
    let mut out = String::new();
    out.push_str(SITES_HEADER);
    out.push('\n');
    for r in rows {
        let ids = r
            .legacy_ids
            .iter()
            .map(|(n, id)| format!("{n}:{id:08X}"))
            .collect::<Vec<_>>()
            .join(";");
        write_record(
            &mut out,
            &[
                &r.send_fn,
                &ids,
                &r.call_sites,
                &r.milestone,
                r.status.tag(),
            ],
        );
    }
    out
}
