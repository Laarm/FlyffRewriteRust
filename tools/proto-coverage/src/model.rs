//! The schema as the checks see it: a plain-data extract of the descriptor set (messages, their custom options, their
//! fields and the two envelopes), so the checks are pure functions that unit tests can feed hand-built fixtures.

use crate::csv::{Dir, Ns};
use anyhow::{Context, Result, bail};
use prost_reflect::{DescriptorPool, ExtensionDescriptor, Kind, MessageDescriptor, Value};
use std::collections::BTreeMap;

/// The wire package.
pub const WIRE_PACKAGE: &str = "flyff.v1";

/// One `(flyff.legacy_id)` option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyId {
    pub ns: Ns,
    pub value: u32,
    pub name: String,
    pub sub: String,
}

/// One field of a message.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub name: String,
    pub number: u32,
    pub is_map: bool,
    /// `int64` / `uint64` / `sint64` / `fixed64` / `sfixed64`.
    pub is_64_bit: bool,
    pub js_safe: bool,
    /// The standard `jstype` option is present (even as JS_NORMAL).
    pub has_jstype: bool,
    pub has_legacy_type: bool,
    pub in_oneof: bool,
}

/// A message with the options the gate reads.
#[derive(Clone, Debug, Default)]
pub struct Message {
    /// `flyff.v1.Name` or `flyff.v1.Outer.Inner`.
    pub full_name: String,
    /// `Name` for a top-level message, `Outer.Inner` for a nested one.
    pub name: String,
    /// Proto file stem (`party`, `guild_war`).
    pub file: String,
    pub nested: bool,
    pub legacy_ids: Vec<LegacyId>,
    pub ext_id: Option<u32>,
    pub stub: bool,
    pub dir: Option<Dir>,
    pub doc: String,
    pub milestone: String,
    pub fields: Vec<Field>,
}

/// One variant of an envelope oneof.
#[derive(Clone, Debug)]
pub struct Case {
    pub tag: u32,
    pub field: String,
    /// Full name of the carried message.
    pub message: String,
}

/// The schema extract.
#[derive(Clone, Debug, Default)]
pub struct Model {
    /// Every message of the wire package, by full name.
    pub messages: BTreeMap<String, Message>,
    /// `ClientEnvelope.msg` variants by ascending tag.
    pub client: Vec<Case>,
    /// `ServerMsg.msg` variants by ascending tag.
    pub server: Vec<Case>,
    /// `reserved` ranges of `ClientEnvelope`, inclusive.
    pub client_reserved: Vec<(u32, u32)>,
    /// `reserved` ranges of `ServerMsg`, inclusive.
    pub server_reserved: Vec<(u32, u32)>,
}

impl Model {
    /// Message by short name (top-level only).
    pub fn top(&self, name: &str) -> Option<&Message> {
        self.messages.get(&format!("{WIRE_PACKAGE}.{name}"))
    }

    /// The envelope variants of a direction.
    pub fn cases(&self, dir: Dir) -> &[Case] {
        match dir {
            Dir::C2S => &self.client,
            Dir::S2C => &self.server,
            Dir::None => &[],
        }
    }

    /// Short names of every envelope message of a direction.
    pub fn envelope_names(&self, dir: Dir) -> Vec<&str> {
        self.cases(dir)
            .iter()
            .filter_map(|c| self.messages.get(&c.message).map(|m| m.name.as_str()))
            .collect()
    }
}

struct Exts {
    legacy_id: ExtensionDescriptor,
    ext_id: ExtensionDescriptor,
    stub: ExtensionDescriptor,
    dir: ExtensionDescriptor,
    doc: ExtensionDescriptor,
    milestone: ExtensionDescriptor,
    legacy_type: ExtensionDescriptor,
    js_safe: ExtensionDescriptor,
}

impl Exts {
    fn load(pool: &DescriptorPool) -> Result<Exts> {
        let get = |n: &str| {
            pool.get_extension_by_name(n)
                .with_context(|| format!("custom option {n} is not defined in the descriptor set"))
        };
        Ok(Exts {
            legacy_id: get("flyff.legacy_id")?,
            ext_id: get("flyff.ext_id")?,
            stub: get("flyff.stub")?,
            dir: get("flyff.dir")?,
            doc: get("flyff.doc")?,
            milestone: get("flyff.milestone")?,
            legacy_type: get("flyff.legacy_type")?,
            js_safe: get("flyff.js_safe_number")?,
        })
    }
}

fn file_stem(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".proto")
        .to_owned()
}

fn is_64(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64
    )
}

fn read_message(desc: &MessageDescriptor, exts: &Exts) -> Result<Message> {
    let opts = desc.options();
    let get_str = |ext: &ExtensionDescriptor| -> String {
        if opts.has_extension(ext)
            && let Value::String(s) = &*opts.get_extension(ext)
        {
            return s.clone();
        }
        String::new()
    };
    let mut legacy_ids = Vec::new();
    if opts.has_extension(&exts.legacy_id) {
        let v = opts.get_extension(&exts.legacy_id);
        let Value::List(items) = &*v else {
            bail!("{}: (flyff.legacy_id) is not a list", desc.full_name());
        };
        for item in items {
            let Value::Message(m) = item else {
                bail!(
                    "{}: (flyff.legacy_id) entry is not a message",
                    desc.full_name()
                );
            };
            let ns = match m.get_field_by_name("ns").as_deref() {
                Some(Value::EnumNumber(1)) => Ns::Packet,
                Some(Value::EnumNumber(2)) => Ns::Snapshot,
                _ => bail!(
                    "{}: (flyff.legacy_id).ns must be PACKETTYPE or SNAPSHOTTYPE",
                    desc.full_name()
                ),
            };
            let value = match m.get_field_by_name("value").as_deref() {
                Some(Value::U32(v)) => *v,
                _ => 0,
            };
            let name = match m.get_field_by_name("name").as_deref() {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            let sub = match m.get_field_by_name("sub").as_deref() {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            legacy_ids.push(LegacyId {
                ns,
                value,
                name,
                sub,
            });
        }
    }
    let ext_id = if opts.has_extension(&exts.ext_id) {
        match &*opts.get_extension(&exts.ext_id) {
            Value::U32(v) => Some(*v),
            _ => None,
        }
    } else {
        None
    };
    let stub = opts.has_extension(&exts.stub)
        && matches!(&*opts.get_extension(&exts.stub), Value::Bool(true));
    let dir = if opts.has_extension(&exts.dir) {
        match &*opts.get_extension(&exts.dir) {
            Value::EnumNumber(1) => Some(Dir::C2S),
            Value::EnumNumber(2) => Some(Dir::S2C),
            _ => None,
        }
    } else {
        None
    };
    let mut fields = Vec::new();
    for f in desc.fields() {
        let fo = f.options();
        fields.push(Field {
            name: f.name().to_owned(),
            number: f.number(),
            is_map: f.is_map(),
            is_64_bit: is_64(&f.kind()),
            js_safe: fo.has_extension(&exts.js_safe)
                && matches!(&*fo.get_extension(&exts.js_safe), Value::Bool(true)),
            has_jstype: fo.has_field_by_name("jstype"),
            has_legacy_type: fo.has_extension(&exts.legacy_type),
            in_oneof: f.containing_oneof().is_some_and(|o| !o.is_synthetic()),
        });
    }
    let full = desc.full_name().to_owned();
    let name = full
        .strip_prefix(&format!("{WIRE_PACKAGE}."))
        .unwrap_or(&full)
        .to_owned();
    Ok(Message {
        full_name: full,
        name,
        file: file_stem(desc.parent_file().name()),
        nested: desc.parent_message().is_some(),
        legacy_ids,
        ext_id,
        stub,
        dir,
        doc: get_str(&exts.doc),
        milestone: get_str(&exts.milestone),
        fields,
    })
}

/// The variants and the reserved ranges of an envelope.
type EnvelopeParts = (Vec<Case>, Vec<(u32, u32)>);

fn envelope(pool: &DescriptorPool, name: &str) -> Result<EnvelopeParts> {
    let full = format!("{WIRE_PACKAGE}.{name}");
    let msg = pool
        .get_message_by_name(&full)
        .with_context(|| format!("{full} is not defined"))?;
    let oneof = msg
        .oneofs()
        .find(|o| o.name() == "msg")
        .with_context(|| format!("{full} has no `oneof msg`"))?;
    let mut cases = Vec::new();
    for f in oneof.fields() {
        let Kind::Message(target) = f.kind() else {
            bail!("{full}.{} is not a message field", f.name());
        };
        cases.push(Case {
            tag: f.number(),
            field: f.name().to_owned(),
            message: target.full_name().to_owned(),
        });
    }
    cases.sort_by_key(|c| c.tag);
    let reserved = msg
        .reserved_ranges()
        .map(|r| (r.start, r.end.saturating_sub(1)))
        .collect();
    Ok((cases, reserved))
}

/// Extracts the model from a descriptor set (`buf build --as-file-descriptor-set` output).
pub fn from_descriptor_set(bytes: &[u8]) -> Result<Model> {
    let pool = DescriptorPool::decode(bytes).context("the descriptor set does not decode")?;
    let exts = Exts::load(&pool)?;
    let mut model = Model::default();
    for desc in pool
        .all_messages()
        .filter(|m| m.package_name() == WIRE_PACKAGE)
    {
        if desc.is_map_entry() {
            continue;
        }
        let m = read_message(&desc, &exts)?;
        model.messages.insert(m.full_name.clone(), m);
    }
    let (client, client_reserved) = envelope(&pool, "ClientEnvelope")?;
    let (server, server_reserved) = envelope(&pool, "ServerMsg")?;
    model.client = client;
    model.client_reserved = client_reserved;
    model.server = server;
    model.server_reserved = server_reserved;
    Ok(model)
}
