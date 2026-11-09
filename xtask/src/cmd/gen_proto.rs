//! `cargo xtask gen-proto`: the protobuf code generation pipeline (Plan/02 §5, Plan/11 §4.3).
//!
//! Steps (all orchestrated here; there is no `build.rs`, no `protoc`, no npm script):
//!
//! 1. `buf format` (check mode: `buf format -d --exit-code`) and `buf lint`,
//! 2. `buf build --as-file-descriptor-set` into a descriptor set,
//! 3. `prost-build` (`compile_fds`, `btree_map`, `bytes`, `enable_type_names`) into
//!    `crates/flyff-proto/src/gen/flyff.v1.rs` and `flyff.rs`,
//! 4. `prost-reflect` over the custom options (`(flyff.legacy_id)`, `(flyff.ext_id)`, `(flyff.max_len)`,
//!    `(flyff.js_safe_number)`) into `legacy_ids.rs`, `from_impls.rs`, `dispatch.rs`, `limits.rs`,
//! 5. `PROTO_HASH` (first 16 hex chars of blake3 over every `proto/**/*.proto`, the admin package included) into
//!    `proto_hash.rs` and `web/src/gen/proto/proto_hash.ts`, plus `web/src/gen/proto/js_safe.ts`,
//! 6. `buf generate` (protoc-gen-es v2) into `web/src/gen/proto/**`,
//! 7. the descriptor set is committed as `crates/flyff-proto/src/gen/descriptor.binpb`.
//!
//! The second package `flyff.admin.v1` (`proto/flyff/admin/v1/admin.proto`, Plan/25 §3.16) goes through steps 2, 3 (prost
//! only: no legacy ids, dispatch, limits or typed registry, which are tables of the wire package) and 6, and its 64-bit
//! fields are checked for `js_safe_number` like the wire package's.
//!
//! Every output is first computed in memory. Without `--check` the outputs are written (and stale files
//! of the two generated directories removed); with `--check` nothing is written and any difference is
//! listed and fails the command.

use crate::proc::{Ctx, Run, banner};
use anyhow::{Context, Result, bail};
use prost::Message as _;
use prost_reflect::{
    DescriptorPool, ExtensionDescriptor, FieldDescriptor, Kind, MessageDescriptor, Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// Arguments of `cargo xtask gen-proto`.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct Args {
    /// `--check`: regenerate in memory and fail on any difference with the committed files.
    #[arg(long)]
    pub check: bool,
}

/// Directory of the `.proto` module (buf.yaml `modules[0].path`).
const PROTO_DIR: &str = "proto";
/// Committed Rust outputs.
const RUST_GEN_DIR: &str = "crates/flyff-proto/src/gen";
/// Generated test helper (committed): the typed prost round trip by message name, see [`rust_typed_registry`].
const TYPED_REGISTRY_PATH: &str = "crates/flyff-proto/tests/gen/typed_registry.rs";
/// Committed TypeScript outputs.
const TS_GEN_DIR: &str = "web/src/gen/proto";
/// Package of the wire messages.
const WIRE_PACKAGE: &str = "flyff.v1";
/// Package of the admin console protocol (`proto/flyff/admin/v1/admin.proto`, Plan/25 §3.16): generated like the wire
/// package (Rust: `flyff.admin.v1.rs`, TypeScript: `flyff/admin/v1/admin_pb.ts` plus the `admin/index.ts` re-export) and
/// part of `PROTO_HASH`, but it has no envelope, no legacy ids and no limits, so none of the tables below cover it.
const ADMIN_PACKAGE: &str = "flyff.admin.v1";

/// `ServerMsg` variants that are boxed so `size_of::<pb::ServerMsg>() <= 256` (Plan/11 §3.4).
const BOXED_TYPES: &[&str] = &[
    "SpawnSelf",
    "SpawnPlayer",
    "SpawnNpc",
    // ItemElem is 248 B inline: an unboxed SpawnItem alone pushes ServerMsg to 288 B (> 256); the two inventory
    // messages that carry a whole template item do the same
    "SpawnItem",
    "CreateItem",
    "PocketAddItem",
    "Guild",
    "AllGuilds",
    "OpenShopWndNtf",
    // economy slice (proto-m4): a restock carries four containers like OpenShopWndNtf; an offered item, a bank
    // item or a mail embeds an ItemElem
    "Vendor",
    "TradePutNtf",
    "PutItemBank",
    "GetItemBank",
    "PostMail",
    "GuildBankWndNtf",
    // guild bank slices that embed an ItemElem (proto-rest)
    "PutItemGuildBankNtf",
    "GetItemGuildBankNtf",
    "QueryMailBoxNtf",
    "HousingAllInfo",
    "Lord",
    "GuildCombat",
];

/// Repository-relative path (always `/`-separated) -> file content.
type Outputs = BTreeMap<String, Vec<u8>>;

macro_rules! wln {
    ($dst:expr) => {{ let _ = writeln!($dst); }};
    ($dst:expr, $($arg:tt)*) => {{ let _ = writeln!($dst, $($arg)*); }};
}

/// Entry point.
pub fn run(ctx: &Ctx, args: &Args) -> Result<()> {
    let total = 6;
    banner("gen-proto", 1, total, "buf format + lint");
    buf_format_and_lint(ctx, args.check)?;

    banner("gen-proto", 2, total, "buf build (descriptor set)");
    let binpb = build_descriptor_set(ctx)?;

    banner("gen-proto", 3, total, "prost-build + reflection tables");
    let mut out = Outputs::new();
    let hash = proto_hash_of_dir(&ctx.path(PROTO_DIR))?;
    generate_rust(&binpb, &hash, &mut out)?;
    generate_ts_static(&binpb, &hash, &mut out)?;

    banner("gen-proto", 4, total, "buf generate (protoc-gen-es)");
    generate_ts_with_buf(ctx, &mut out)?;

    banner(
        "gen-proto",
        5,
        total,
        if args.check {
            "compare with the committed files"
        } else {
            "write the generated files"
        },
    );
    if args.check {
        let stale = diff_outputs(ctx, &out)?;
        if !stale.is_empty() {
            for s in &stale {
                eprintln!("  stale: {s}");
            }
            bail!(
                "{} generated protobuf file(s) are out of date; run `cargo xtask gen-proto` and commit the result",
                stale.len()
            );
        }
        println!("    {} generated files are up to date", out.len());
    } else {
        let written = write_outputs(ctx, &out)?;
        println!(
            "    {} files generated, {written} changed (PROTO_HASH {hash})",
            out.len()
        );
    }
    banner("gen-proto", 6, total, "done");
    println!("gen-proto: OK");
    Ok(())
}

/// The `gen --check` hook (Plan/02 §4.2, §9.8): runs `gen-proto --check` (or, without `--check`, the
/// generation) when `buf` and the pnpm-installed `protoc-gen-es` are available, and prints a skip note
/// otherwise so machines without them can still run `cargo xtask check`.
pub fn gen_if_available(ctx: &Ctx, check: bool) -> Result<()> {
    let have_buf = Run::new(ctx, "buf")
        .arg("--version")
        .capture()
        .is_ok_and(|(ok, _, _)| ok);
    let have_plugin = ctx.path("web/node_modules/.bin/protoc-gen-es").exists();
    if !(have_buf && have_plugin) || !ctx.path("proto").is_dir() {
        println!(
            "    gen-proto: skipped (buf: {}, web/node_modules protoc-gen-es: {})",
            if have_buf { "found" } else { "missing" },
            if have_plugin {
                "found"
            } else {
                "missing; pnpm --dir web install"
            },
        );
        return Ok(());
    }
    run(ctx, &Args { check })
}

// ---------------------------------------------------------------------------------------------------
// buf
// ---------------------------------------------------------------------------------------------------

fn buf_format_and_lint(ctx: &Ctx, check: bool) -> Result<()> {
    if check {
        Run::new(ctx, "buf")
            .args(["format", "-d", "--exit-code"])
            .run()
            .context("the .proto files are not buf-formatted; run `cargo xtask gen-proto`")?;
    } else {
        Run::new(ctx, "buf").args(["format", "-w"]).run()?;
    }
    Run::new(ctx, "buf").arg("lint").run()
}

fn build_descriptor_set(ctx: &Ctx) -> Result<Vec<u8>> {
    let dir = tempfile::tempdir().context("temp dir")?;
    let path = dir.path().join("flyff.binpb");
    Run::new(ctx, "buf")
        .args([
            "build",
            "--as-file-descriptor-set",
            "--exclude-source-info",
            "-o",
        ])
        .arg(&path)
        .run()?;
    std::fs::read(&path).with_context(|| format!("read {}", path.display()))
}

/// Runs `buf generate --output <tmp>` and adds every produced file under `web/src/gen/proto/`.
fn generate_ts_with_buf(ctx: &Ctx, out: &mut Outputs) -> Result<()> {
    if !ctx.path("web/node_modules/.bin/protoc-gen-es").exists() {
        bail!(
            "web/node_modules/.bin/protoc-gen-es is missing: run `pnpm --dir web install --frozen-lockfile` first"
        );
    }
    let tmp = tempfile::tempdir().context("temp dir")?;
    Run::new(ctx, "buf")
        .args(["generate", "--output"])
        .arg(tmp.path())
        .run()?;
    let base = tmp.path().join(TS_GEN_DIR);
    if !base.is_dir() {
        bail!("buf generate produced no {TS_GEN_DIR}");
    }
    for entry in walkdir::WalkDir::new(&base).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&base)
            .context("strip prefix")?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(entry.path())?;
        out.insert(format!("{TS_GEN_DIR}/{rel}"), bytes);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------
// PROTO_HASH (Plan/02 §5 step 5, Plan/11 §4.4)
// ---------------------------------------------------------------------------------------------------

/// First 16 lowercase hex chars of `blake3( path ‖ 0x00 ‖ content(LF) ‖ 0x00 …)` over the sorted
/// `(module-relative path, content)` pairs.
pub fn proto_hash(files: &[(String, Vec<u8>)]) -> String {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = blake3::Hasher::new();
    for (path, content) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(&normalise_lf(content));
        hasher.update(&[0]);
    }
    let digest = hasher.finalize();
    digest.to_hex().as_str().chars().take(16).collect()
}

fn normalise_lf(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut i = 0;
    while let Some(&b) = content.get(i) {
        if b == b'\r' && content.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    out
}

fn proto_hash_of_dir(dir: &Path) -> Result<String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "proto") {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .context("strip prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((rel, std::fs::read(entry.path())?));
        }
    }
    if files.is_empty() {
        bail!("no .proto files under {}", dir.display());
    }
    Ok(proto_hash(&files))
}

// ---------------------------------------------------------------------------------------------------
// The schema model read from the descriptor set
// ---------------------------------------------------------------------------------------------------

/// One variant of an envelope oneof.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Variant {
    /// Proto field name (`hello_ack`); the handler method is `on_<field>`.
    field: String,
    /// Wire tag.
    tag: u32,
    /// Message type name (top-level message of the wire package).
    ty: String,
    /// The prost variant is `Box<T>`.
    boxed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LegacyRow {
    message: String,
    ns: &'static str,
    value: u32,
    name: String,
    sub: String,
    dir: &'static str,
    ext_id: u32,
}

#[derive(Debug, Default)]
struct Schema {
    client: Vec<Variant>,
    server: Vec<Variant>,
    legacy: Vec<LegacyRow>,
    ext_ids: Vec<(String, u32)>,
    js_safe: Vec<String>,
    unsafe_64: Vec<String>,
}

struct Exts {
    legacy_id: ExtensionDescriptor,
    ext_id: ExtensionDescriptor,
    dir: ExtensionDescriptor,
    max_len: ExtensionDescriptor,
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
            dir: get("flyff.dir")?,
            max_len: get("flyff.max_len")?,
            js_safe: get("flyff.js_safe_number")?,
        })
    }
}

fn wire_messages(pool: &DescriptorPool) -> Vec<MessageDescriptor> {
    let mut v: Vec<MessageDescriptor> = pool
        .all_messages()
        .filter(|m| m.package_name() == WIRE_PACKAGE)
        .collect();
    v.sort_by(|a, b| a.full_name().cmp(b.full_name()));
    v
}

fn envelope_variants(pool: &DescriptorPool, envelope: &str) -> Result<Vec<Variant>> {
    let full = format!("{WIRE_PACKAGE}.{envelope}");
    let msg = pool
        .get_message_by_name(&full)
        .with_context(|| format!("{full} is not defined"))?;
    let oneof = msg
        .oneofs()
        .find(|o| o.name() == "msg")
        .with_context(|| format!("{full} has no `oneof msg`"))?;
    let mut out = Vec::new();
    for f in oneof.fields() {
        let Kind::Message(target) = f.kind() else {
            bail!("{full}.{} is not a message field", f.name());
        };
        if target.parent_message().is_some() || target.package_name() != WIRE_PACKAGE {
            bail!(
                "{full}.{}: envelope variants must be top-level {WIRE_PACKAGE} messages",
                f.name()
            );
        }
        out.push(Variant {
            field: f.name().to_owned(),
            tag: f.number(),
            ty: target.name().to_owned(),
            boxed: envelope == "ServerMsg" && BOXED_TYPES.contains(&target.name()),
        });
    }
    out.sort_by_key(|v| v.tag);
    Ok(out)
}

fn is_64_bit(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Int64 | Kind::Uint64 | Kind::Sint64 | Kind::Fixed64 | Kind::Sfixed64
    )
}

fn opt_u32(opts: &prost_reflect::DynamicMessage, ext: &ExtensionDescriptor) -> Option<u32> {
    if !opts.has_extension(ext) {
        return None;
    }
    match &*opts.get_extension(ext) {
        Value::U32(v) => Some(*v),
        _ => None,
    }
}

fn opt_bool(opts: &prost_reflect::DynamicMessage, ext: &ExtensionDescriptor) -> bool {
    opts.has_extension(ext) && matches!(&*opts.get_extension(ext), Value::Bool(true))
}

fn read_schema(pool: &DescriptorPool) -> Result<Schema> {
    let exts = Exts::load(pool)?;
    let mut schema = Schema {
        client: envelope_variants(pool, "ClientEnvelope")?,
        server: envelope_variants(pool, "ServerMsg")?,
        ..Schema::default()
    };
    let client_types: BTreeSet<&str> = schema.client.iter().map(|v| v.ty.as_str()).collect();
    let server_types: BTreeSet<&str> = schema.server.iter().map(|v| v.ty.as_str()).collect();

    for msg in wire_messages(pool) {
        let opts = msg.options();
        let ext_id = opt_u32(&opts, &exts.ext_id).unwrap_or(0);
        if ext_id != 0 {
            if !(0xFE00_0000..=0xFEFF_FFFF).contains(&ext_id) {
                bail!(
                    "{}: (flyff.ext_id) 0x{ext_id:08X} is outside 0xFE000000..0xFEFFFFFF",
                    msg.full_name()
                );
            }
            schema.ext_ids.push((msg.full_name().to_owned(), ext_id));
        }
        let declared_dir = if opts.has_extension(&exts.dir) {
            match &*opts.get_extension(&exts.dir) {
                Value::EnumNumber(1) => Some("Direction::ClientToServer"),
                Value::EnumNumber(2) => Some("Direction::ServerToClient"),
                _ => None,
            }
        } else {
            None
        };
        let in_client = client_types.contains(msg.name());
        let in_server = server_types.contains(msg.name());
        let dir = match (declared_dir, in_client, in_server) {
            (Some(d), _, _) => Some(d),
            (None, true, false) => Some("Direction::ClientToServer"),
            (None, false, true) => Some("Direction::ServerToClient"),
            _ => None,
        };
        if let Some(declared) = declared_dir {
            let consistent = match declared {
                "Direction::ClientToServer" => !in_server || in_client,
                _ => !in_client || in_server,
            };
            if !consistent {
                bail!(
                    "{}: (flyff.dir) contradicts its membership in the envelopes",
                    msg.full_name()
                );
            }
        }
        if opts.has_extension(&exts.legacy_id) {
            let Some(dir) = dir else {
                bail!(
                    "{}: carries (flyff.legacy_id) but has no direction (declare (flyff.dir) or put it in an envelope)",
                    msg.full_name()
                );
            };
            let value = opts.get_extension(&exts.legacy_id);
            let Value::List(items) = &*value else {
                bail!("{}: (flyff.legacy_id) is not a list", msg.full_name());
            };
            for item in items {
                let Value::Message(m) = item else {
                    bail!(
                        "{}: (flyff.legacy_id) entry is not a message",
                        msg.full_name()
                    );
                };
                let ns = match m.get_field_by_name("ns").as_deref() {
                    Some(Value::EnumNumber(1)) => "LegacyNamespace::Packet",
                    Some(Value::EnumNumber(2)) => "LegacyNamespace::Snapshot",
                    _ => bail!(
                        "{}: (flyff.legacy_id).ns must be PACKETTYPE or SNAPSHOTTYPE",
                        msg.full_name()
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
                if name.is_empty() {
                    bail!("{}: (flyff.legacy_id).name is empty", msg.full_name());
                }
                schema.legacy.push(LegacyRow {
                    message: msg.full_name().to_owned(),
                    ns,
                    value,
                    name,
                    sub,
                    dir,
                    ext_id,
                });
            }
        }
        for f in msg.fields() {
            if !is_64_bit(&f.kind()) {
                continue;
            }
            let full = format!("{}.{}", msg.full_name(), f.name());
            if opt_bool(&f.options(), &exts.js_safe) {
                schema.js_safe.push(full);
            } else {
                schema.unsafe_64.push(full);
            }
        }
    }
    // The admin protocol (Plan/25 §3.16): only its 64-bit fields take part (they must carry the same
    // `js_safe_number` marker; the web client converts them with `n64`).
    for msg in pool
        .all_messages()
        .filter(|m| m.package_name() == ADMIN_PACKAGE)
    {
        for f in msg.fields() {
            if !is_64_bit(&f.kind()) {
                continue;
            }
            let full = format!("{}.{}", msg.full_name(), f.name());
            if opt_bool(&f.options(), &exts.js_safe) {
                schema.js_safe.push(full);
            } else {
                schema.unsafe_64.push(full);
            }
        }
    }
    schema.legacy.sort_by(|a, b| {
        (&a.message, a.value, &a.name, &a.sub).cmp(&(&b.message, b.value, &b.name, &b.sub))
    });
    schema.js_safe.sort();
    schema.unsafe_64.sort();
    Ok(schema)
}

// ---------------------------------------------------------------------------------------------------
// Rust outputs
// ---------------------------------------------------------------------------------------------------

const RUST_HEADER: &str =
    "// @generated by `cargo xtask gen-proto` (Plan/02 section 5). Do not edit.\n";
const TS_HEADER: &str =
    "// @generated by `cargo xtask gen-proto` (Plan/02 section 5). Do not edit.\n";

fn generate_rust(binpb: &[u8], hash: &str, out: &mut Outputs) -> Result<()> {
    let pool =
        DescriptorPool::decode(binpb).context("decode the descriptor set (prost-reflect)")?;
    let schema = read_schema(&pool)?;
    if !schema.unsafe_64.is_empty() {
        println!(
            "    note: {} 64-bit field(s) without (flyff.js_safe_number): {}",
            schema.unsafe_64.len(),
            schema.unsafe_64.join(", ")
        );
    }

    for (name, bytes) in prost_outputs(binpb, &schema)? {
        out.insert(format!("{RUST_GEN_DIR}/{name}"), bytes);
    }
    out.insert(format!("{RUST_GEN_DIR}/descriptor.binpb"), binpb.to_vec());
    let mut put = |name: &str, text: String| {
        out.insert(format!("{RUST_GEN_DIR}/{name}"), text.into_bytes());
    };
    put("proto_hash.rs", rust_proto_hash(hash));
    put("legacy_ids.rs", rust_legacy_ids(&schema));
    put("from_impls.rs", rust_from_impls(&schema));
    put("dispatch.rs", rust_dispatch(&schema));
    put("limits.rs", rust_limits(&pool, &schema)?);
    // test-only: not part of the library, included by crates/flyff-proto/tests/golden_messages.rs
    out.insert(
        TYPED_REGISTRY_PATH.to_owned(),
        rust_typed_registry(&pool).into_bytes(),
    );
    Ok(())
}

/// Runs prost-build on the descriptor set and returns `(file name, bytes)` of every produced file.
fn prost_outputs(binpb: &[u8], schema: &Schema) -> Result<Vec<(String, Vec<u8>)>> {
    let mut fds =
        prost_types::FileDescriptorSet::decode(binpb).context("decode the descriptor set")?;
    pack_repeated_scalars(&mut fds);
    let tmp = tempfile::tempdir().context("temp dir")?;
    let mut cfg = prost_build::Config::new();
    cfg.out_dir(tmp.path());
    cfg.btree_map(["."]);
    cfg.bytes(["."]);
    cfg.enable_type_names();
    // `.google.protobuf` is mapped to `::prost_types` by prost-build itself, so the descriptor.proto
    // import of options.proto (only needed for the `extend` blocks) generates no code.
    for v in schema.server.iter().filter(|v| v.boxed) {
        // prost addresses a oneof variant as `.<package>.<Message>.<oneof>.<field>`
        cfg.boxed(format!(".{WIRE_PACKAGE}.ServerMsg.msg.{}", v.field));
    }
    cfg.compile_fds(fds).context("prost-build compile_fds")?;
    let mut files = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(tmp.path())?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let bytes = std::fs::read(tmp.path().join(&name))?;
        files.push((name, bytes));
    }
    Ok(files)
}

/// prost-build 0.14 decides "packed" with `options.map_or(proto3, |o| o.packed())`: a repeated scalar field that
/// carries ANY option (our `(flyff.legacy_type)`, `(flyff.max_len)` and `(flyff.js_safe_number)` all count, even
/// though prost drops the extension values) is treated as explicitly unpacked, so the generated Rust type would
/// encode `repeated uint32 index` as one key per element while protobuf-es and the reflection encoder write the
/// proto3 default (one packed field). Wire-compatible but different bytes and up to 2x larger, so the golden
/// vectors of WP-PROTO-07 failed. Mark every proto3 repeated numeric/bool/enum field that has an options message
/// but no explicit `packed` as `packed = true` (the proto3 default) before the descriptor reaches prost-build.
fn pack_repeated_scalars(fds: &mut prost_types::FileDescriptorSet) {
    use prost_types::field_descriptor_proto::{Label, Type};

    fn walk(msg: &mut prost_types::DescriptorProto) {
        for field in &mut msg.field {
            let repeated = field.label == Some(Label::Repeated as i32);
            let packable = !matches!(
                Type::try_from(field.r#type.unwrap_or_default()),
                Ok(Type::String | Type::Bytes | Type::Message | Type::Group) | Err(_)
            );
            if let (true, true, Some(options)) = (repeated, packable, field.options.as_mut())
                && options.packed.is_none()
            {
                options.packed = Some(true);
            }
        }
        for nested in &mut msg.nested_type {
            walk(nested);
        }
    }

    for file in &mut fds.file {
        if file.syntax.as_deref() == Some("proto3") {
            for msg in &mut file.message_type {
                walk(msg);
            }
        }
    }
}

fn rust_proto_hash(hash: &str) -> String {
    format!(
        "{RUST_HEADER}\n/// Hash of all `.proto` sources: first 16 hex chars of blake3 over every `proto/**/*.proto`\n/// sorted by path (`path 0x00 content 0x00`, LF line ends); sent in `Hello.proto_hash` (11 section 4.4).\npub const PROTO_HASH: &str = \"{hash}\";\n"
    )
}

fn rust_legacy_ids(schema: &Schema) -> String {
    let mut s = String::from(RUST_HEADER);
    wln!(s);
    wln!(
        s,
        "/// Legacy packet/snapshot ids of the schema, sorted by message full name (11 section 4.3)."
    );
    wln!(s, "pub static LEGACY_IDS: &[LegacyIdEntry] = &[");
    for r in &schema.legacy {
        wln!(
            s,
            "    LegacyIdEntry {{ message: {:?}, ns: {}, value: 0x{:X}, name: {:?}, sub: {:?}, dir: {}, ext_id: 0x{:X} }},",
            r.message,
            r.ns,
            r.value,
            r.name,
            r.sub,
            r.dir,
            r.ext_id
        );
    }
    wln!(s, "];");
    wln!(s);
    wln!(
        s,
        "/// Extension ids (`(flyff.ext_id)`, 0xFE000000 and up) of the schema: `(message full name, id)`, sorted by name."
    );
    wln!(s, "pub static EXT_IDS: &[(&str, u32)] = &[");
    let mut ext = schema.ext_ids.clone();
    ext.sort();
    for (m, id) in ext {
        wln!(s, "    ({m:?}, 0x{id:X}),");
    }
    wln!(s, "];");
    s
}

/// prost-build's `sanitize_identifier` (ident.rs): raw identifiers for keywords, a suffix for the few
/// keywords that cannot be raw, a prefix for names starting with a digit.
fn sanitize_identifier(ident: &str) -> String {
    match ident {
        "as" | "break" | "const" | "continue" | "else" | "enum" | "false" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref"
        | "return" | "static" | "struct" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro"
        | "override" | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "async" | "await"
        | "try" | "gen" => {
            format!("r#{ident}")
        }
        "_" | "super" | "self" | "Self" | "extern" | "crate" => format!("{ident}_"),
        s if s.starts_with(|c: char| c.is_numeric()) => format!("_{ident}"),
        _ => ident.to_owned(),
    }
}

/// Rust field / module identifier prost derives from a proto name (`to_snake` of prost-build).
fn rust_snake(name: &str) -> String {
    use heck::ToSnakeCase as _;
    sanitize_identifier(&name.to_snake_case())
}

/// Rust type / enum-variant identifier prost derives from a proto name (`to_upper_camel`).
fn upper_camel(name: &str) -> String {
    use heck::ToUpperCamelCase as _;
    sanitize_identifier(&name.to_upper_camel_case())
}

/// `pb::outer` / `pb::outer::inner`: the module prost generates for the nested items of a message
/// (its oneof enums, nested messages and enums).
fn rust_module_path(msg: &MessageDescriptor) -> String {
    let path = rust_type_path(msg);
    match path.rsplit_once("::") {
        Some((prefix, _)) => format!("{prefix}::{}", rust_snake(msg.name())),
        None => rust_snake(msg.name()),
    }
}

/// `pb::Outer` / `pb::outer::Inner`: where prost puts a message of the wire package.
fn rust_type_path(msg: &MessageDescriptor) -> String {
    let relative = msg
        .full_name()
        .strip_prefix(&format!("{WIRE_PACKAGE}."))
        .unwrap_or_else(|| msg.full_name());
    let mut parts: Vec<&str> = relative.split('.').collect();
    let last = parts.pop().unwrap_or_default();
    let mut path = String::from("pb");
    for module in parts {
        path.push_str("::");
        path.push_str(&rust_snake(module));
    }
    path.push_str("::");
    path.push_str(&upper_camel(last));
    path
}

fn variant_ctor(v: &Variant, enum_path: &str) -> String {
    let name = upper_camel(&v.field);
    if v.boxed {
        format!("{enum_path}::{name}(Box::new(v))")
    } else {
        format!("{enum_path}::{name}(v)")
    }
}

fn rust_from_impls(schema: &Schema) -> String {
    let mut s = String::from(RUST_HEADER);
    wln!(s);
    wln!(
        s,
        "// `From<T>` for both envelope oneofs, so game code writes `out.to_session(sid, 0, pb::Kick {{ .. }})` (11 section 3.4)."
    );
    for (variants, path) in [
        (&schema.client, "pb::client_envelope::Msg"),
        (&schema.server, "pb::server_msg::Msg"),
    ] {
        for v in variants {
            wln!(s);
            wln!(s, "impl From<pb::{}> for {path} {{", v.ty);
            wln!(s, "    fn from(v: pb::{}) -> Self {{", v.ty);
            wln!(s, "        {}", variant_ctor(v, "Self"));
            wln!(s, "    }}");
            wln!(s, "}}");
        }
    }
    s
}

fn rust_dispatch(schema: &Schema) -> String {
    let mut s = String::from(RUST_HEADER);
    wln!(s);
    wln!(
        s,
        "/// Handlers for every client-to-server envelope message (11 section 3.7). Every method defaults to"
    );
    wln!(s, "/// [`ClientHandlers::unhandled`].");
    wln!(s, "pub trait ClientHandlers {{");
    wln!(
        s,
        "    /// Session handle of the implementor (flyff-game binds its `SessionId`; this crate stays dependency-free)."
    );
    wln!(s, "    type Sid: Copy;");
    for v in &schema.client {
        wln!(s, "    /// Handles `{}`.", v.ty);
        wln!(
            s,
            "    fn on_{}(&mut self, sid: Self::Sid, _m: pb::{}) {{",
            v.field,
            v.ty
        );
        wln!(s, "        self.unhandled(sid, {:?});", v.ty);
        wln!(s, "    }}");
    }
    wln!(
        s,
        "    /// Called for every message without an override (log at debug level and count per name)."
    );
    wln!(
        s,
        "    fn unhandled(&mut self, sid: Self::Sid, name: &'static str);"
    );
    wln!(s, "}}");
    wln!(s);
    wln!(
        s,
        "/// Routes one decoded client message to its handler method."
    );
    wln!(
        s,
        "pub fn dispatch_client<H: ClientHandlers>(h: &mut H, sid: H::Sid, msg: pb::client_envelope::Msg) {{"
    );
    wln!(s, "    match msg {{");
    for v in &schema.client {
        let name = upper_camel(&v.field);
        let arg = if v.boxed { "*m" } else { "m" };
        wln!(
            s,
            "        pb::client_envelope::Msg::{name}(m) => h.on_{}(sid, {arg}),",
            v.field
        );
    }
    wln!(s, "    }}");
    wln!(s, "}}");
    wln!(s);
    wln!(
        s,
        "/// Handlers for every server-to-client envelope message (the Rust bot client, D9)."
    );
    wln!(s, "pub trait ServerHandlers {{");
    for v in &schema.server {
        wln!(s, "    /// Handles `{}`.", v.ty);
        wln!(
            s,
            "    fn on_{}(&mut self, obj_id: u32, _m: pb::{}) {{",
            v.field,
            v.ty
        );
        wln!(s, "        self.unhandled(obj_id, {:?});", v.ty);
        wln!(s, "    }}");
    }
    wln!(s, "    /// Called for every message without an override.");
    wln!(
        s,
        "    fn unhandled(&mut self, obj_id: u32, name: &'static str);"
    );
    wln!(s, "}}");
    wln!(s);
    wln!(
        s,
        "/// Routes one decoded server message to its handler method."
    );
    wln!(
        s,
        "pub fn dispatch_server<H: ServerHandlers>(h: &mut H, obj_id: u32, msg: pb::server_msg::Msg) {{"
    );
    wln!(s, "    match msg {{");
    for v in &schema.server {
        let name = upper_camel(&v.field);
        let arg = if v.boxed { "*m" } else { "m" };
        wln!(
            s,
            "        pb::server_msg::Msg::{name}(m) => h.on_{}(obj_id, {arg}),",
            v.field
        );
    }
    wln!(s, "    }}");
    wln!(s, "}}");
    for (variants, name, what) in [
        (&schema.client, "CLIENT_MSG_NAMES", "`ClientEnvelope`"),
        (&schema.server, "SERVER_MSG_NAMES", "`ServerMsg`"),
    ] {
        wln!(s);
        wln!(
            s,
            "/// {what} oneof tag -> message name, ascending tag order (per-message metrics, 10 section 7.4)."
        );
        wln!(s, "pub static {name}: &[(u32, &str)] = &[");
        for v in variants {
            wln!(s, "    ({}, {:?}),", v.tag, v.ty);
        }
        wln!(s, "];");
    }
    // Fieldless discriminants of both oneofs: `MsgCase` (ServerMsg, WP-WORLD-04 `OutboxCapture` helpers and
    // the gateway counters) and `ClientCase` (per-tag inbound counters, 10 section 7.4).
    for (variants, case, msg_path, what) in [
        (
            &schema.client,
            "ClientCase",
            "pb::client_envelope::Msg",
            "`ClientEnvelope`",
        ),
        (
            &schema.server,
            "MsgCase",
            "pb::server_msg::Msg",
            "`ServerMsg`",
        ),
    ] {
        wln!(s);
        wln!(
            s,
            "/// Fieldless discriminant of the {what} oneof: one variant per message, in tag order."
        );
        wln!(
            s,
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]"
        );
        wln!(s, "pub enum {case} {{");
        for v in variants {
            wln!(s, "    /// `{}`.", v.ty);
            wln!(s, "    {},", upper_camel(&v.field));
        }
        wln!(s, "}}");
        wln!(s);
        wln!(s, "impl {case} {{");
        wln!(s, "    /// Every case, ascending by tag.");
        wln!(s, "    pub const ALL: &'static [{case}] = &[");
        for v in variants {
            wln!(s, "        {case}::{},", upper_camel(&v.field));
        }
        wln!(s, "    ];");
        wln!(s);
        wln!(s, "    /// The oneof tag of this case.");
        wln!(s, "    pub const fn tag(self) -> u32 {{");
        wln!(s, "        match self {{");
        for v in variants {
            wln!(
                s,
                "            {case}::{} => {},",
                upper_camel(&v.field),
                v.tag
            );
        }
        wln!(s, "        }}");
        wln!(s, "    }}");
        wln!(s);
        wln!(
            s,
            "    /// The message type name (as in the `*_MSG_NAMES` table)."
        );
        wln!(s, "    pub const fn name(self) -> &'static str {{");
        wln!(s, "        match self {{");
        for v in variants {
            wln!(
                s,
                "            {case}::{} => {:?},",
                upper_camel(&v.field),
                v.ty
            );
        }
        wln!(s, "        }}");
        wln!(s, "    }}");
        wln!(s, "}}");
        wln!(s);
        wln!(s, "impl {msg_path} {{");
        wln!(s, "    /// The fieldless discriminant of this message.");
        wln!(s, "    pub const fn case(&self) -> {case} {{");
        wln!(s, "        match self {{");
        for v in variants {
            wln!(
                s,
                "            Self::{}(_) => {case}::{},",
                upper_camel(&v.field),
                upper_camel(&v.field)
            );
        }
        wln!(s, "        }}");
        wln!(s, "    }}");
        wln!(s, "}}");
    }
    s
}

// ---- typed registry (tests) ------------------------------------------------------------------------

/// `roundtrip_typed(full_name, bytes)`: decodes `bytes` with the prost type of every wire message (nested
/// ones included) and encodes the value again. The golden-vector test (WP-PROTO-07) uses it to prove that the
/// production encoder emits byte-identical output to the reflection encoder and to protobuf-es, without a
/// hand-maintained list of types (parallel WPs only add `.json` vectors).
fn rust_typed_registry(pool: &DescriptorPool) -> String {
    let mut s = String::from(RUST_HEADER);
    wln!(s);
    wln!(
        s,
        "// Included by tests/golden_messages.rs (`use flyff_proto::pb;` must be in scope)."
    );
    wln!(s);
    wln!(
        s,
        "/// Decodes `bytes` as the wire message `full_name` and encodes it again; `None` for an unknown name."
    );
    wln!(
        s,
        "pub fn roundtrip_typed(full_name: &str, bytes: &[u8]) -> Option<Result<Vec<u8>, prost::DecodeError>> {{"
    );
    wln!(s, "    use prost::Message as _;");
    wln!(s, "    match full_name {{");
    for m in wire_messages(pool) {
        wln!(
            s,
            "        {:?} => Some({}::decode(bytes).map(|m| m.encode_to_vec())),",
            m.full_name(),
            rust_type_path(&m)
        );
    }
    wln!(s, "        _ => None,");
    wln!(s, "    }}");
    wln!(s, "}}");
    s
}

// ---- limits ---------------------------------------------------------------------------------------

/// `true` when `check_limits` of this message can ever fail (it or a nested message has a cap).
fn has_limits(msg: &MessageDescriptor, exts: &Exts, seen: &mut BTreeSet<String>) -> bool {
    if !seen.insert(msg.full_name().to_owned()) {
        return false;
    }
    msg.fields().any(|f| {
        opt_u32(&f.options(), &exts.max_len).is_some()
            || match f.kind() {
                Kind::Message(m) => has_limits(&m, exts, seen),
                _ => false,
            }
    })
}

/// How the generated code reaches the value of a field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `self.f`: the prost field type itself (`String`, `Vec<T>`, `Option<M>` for message fields).
    Field,
    /// `self.f` of a proto3 `optional` scalar: `Option<String>` etc.
    Optional,
    /// The payload of a `oneof` variant, bound as `v: &T`.
    Variant,
}

fn limit_lines(msg: &MessageDescriptor, exts: &Exts) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for f in msg.fields() {
        if f.is_map() {
            bail!(
                "{}.{}: map fields are not allowed (Plan/11 section 5.2)",
                msg.full_name(),
                f.name()
            );
        }
        let shape = match f.containing_oneof() {
            Some(o) if o.is_synthetic() => Shape::Optional,
            Some(_) => continue, // handled below, per oneof
            None => Shape::Field,
        };
        let access = format!("self.{}", rust_snake(f.name()));
        let label = format!("{}.{}", msg.name(), f.name());
        let max = opt_u32(&f.options(), &exts.max_len);
        lines.extend(field_limit_lines(&f, &access, &label, max, shape, exts)?);
    }
    for oneof in msg.oneofs().filter(|o| !o.is_synthetic()) {
        let enum_path = format!("{}::{}", rust_module_path(msg), upper_camel(oneof.name()));
        let mut arms = Vec::new();
        for f in oneof.fields() {
            let label = format!("{}.{}", msg.name(), f.name());
            let max = opt_u32(&f.options(), &exts.max_len);
            let body = field_limit_lines(&f, "v", &label, max, Shape::Variant, exts)?;
            if !body.is_empty() {
                arms.push((upper_camel(f.name()), body));
            }
        }
        if arms.is_empty() {
            continue;
        }
        lines.push(format!("match &self.{} {{", rust_snake(oneof.name())));
        for (variant, body) in arms {
            lines.push(format!("    Some({enum_path}::{variant}(v)) => {{"));
            for l in body {
                lines.push(format!("        {l}"));
            }
            lines.push("    }".to_owned());
        }
        lines.push("    _ => {}".to_owned());
        lines.push("}".to_owned());
    }
    Ok(lines)
}

fn field_limit_lines(
    f: &FieldDescriptor,
    access: &str,
    label: &str,
    max: Option<u32>,
    shape: Shape,
    exts: &Exts,
) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    if f.is_list() {
        if let Some(max) = max {
            lines.push(format!(
                "crate::limits::check_count({label:?}, {access}.len(), {max})?;"
            ));
        }
        if let Kind::Message(m) = f.kind()
            && has_limits(&m, exts, &mut BTreeSet::new())
        {
            lines.push(format!("for item in &{access} {{ item.check_limits()?; }}"));
        }
        return Ok(lines);
    }
    match f.kind() {
        Kind::String | Kind::Bytes => {
            if let Some(max) = max {
                let check = |value: &str| {
                    format!("crate::limits::check_bytes({label:?}, {value}.len(), {max})?;")
                };
                match shape {
                    Shape::Field | Shape::Variant => lines.push(check(access)),
                    Shape::Optional => {
                        lines.push(format!("if let Some(v) = &{access} {{ {} }}", check("v")))
                    }
                }
            }
        }
        Kind::Message(m) => {
            if max.is_some() {
                bail!("{label}: (flyff.max_len) on a singular message field");
            }
            if has_limits(&m, exts, &mut BTreeSet::new()) {
                lines.push(match shape {
                    Shape::Variant => format!("{access}.check_limits()?;"),
                    Shape::Field | Shape::Optional => {
                        format!("if let Some(inner) = &{access} {{ inner.check_limits()?; }}")
                    }
                });
            }
        }
        _ => {
            if max.is_some() {
                bail!(
                    "{label}: (flyff.max_len) is only valid on string, bytes and repeated fields"
                );
            }
        }
    }
    Ok(lines)
}

fn rust_limits(pool: &DescriptorPool, schema: &Schema) -> Result<String> {
    let exts = Exts::load(pool)?;
    // Every message reachable from a client-to-server envelope message.
    let mut reachable: BTreeMap<String, MessageDescriptor> = BTreeMap::new();
    let mut stack: Vec<MessageDescriptor> = Vec::new();
    for v in &schema.client {
        let m = pool
            .get_message_by_name(&format!("{WIRE_PACKAGE}.{}", v.ty))
            .with_context(|| format!("message {} not found", v.ty))?;
        stack.push(m);
    }
    while let Some(m) = stack.pop() {
        if reachable.contains_key(m.full_name()) {
            continue;
        }
        for f in m.fields() {
            if let Kind::Message(t) = f.kind()
                && t.package_name() == WIRE_PACKAGE
            {
                stack.push(t);
            }
        }
        reachable.insert(m.full_name().to_owned(), m);
    }

    let mut s = String::from(RUST_HEADER);
    wln!(s);
    wln!(
        s,
        "// `Limits` for every message reachable from `ClientEnvelope`, from `(flyff.max_len)` (11 section 5.7, 6)."
    );
    wln!(s);
    wln!(s, "impl Limits for pb::client_envelope::Msg {{");
    wln!(
        s,
        "    fn check_limits(&self) -> Result<(), LimitViolation> {{"
    );
    wln!(s, "        match self {{");
    for v in &schema.client {
        wln!(
            s,
            "            pb::client_envelope::Msg::{}(m) => m.check_limits(),",
            upper_camel(&v.field)
        );
    }
    wln!(s, "        }}");
    wln!(s, "    }}");
    wln!(s, "}}");
    wln!(s);
    wln!(s, "impl Limits for pb::ClientEnvelope {{");
    wln!(
        s,
        "    fn check_limits(&self) -> Result<(), LimitViolation> {{"
    );
    wln!(s, "        match &self.msg {{");
    wln!(s, "            Some(m) => m.check_limits(),");
    wln!(s, "            None => Ok(()),");
    wln!(s, "        }}");
    wln!(s, "    }}");
    wln!(s, "}}");
    for m in reachable.values() {
        if m.name() == "ClientEnvelope" {
            continue;
        }
        let lines = limit_lines(m, &exts)?;
        wln!(s);
        wln!(s, "impl Limits for {} {{", rust_type_path(m));
        wln!(
            s,
            "    fn check_limits(&self) -> Result<(), LimitViolation> {{"
        );
        for l in lines {
            wln!(s, "        {l}");
        }
        wln!(s, "        Ok(())");
        wln!(s, "    }}");
        wln!(s, "}}");
    }
    Ok(s)
}

// ---------------------------------------------------------------------------------------------------
// TypeScript outputs that do not come from buf
// ---------------------------------------------------------------------------------------------------

fn generate_ts_static(binpb: &[u8], hash: &str, out: &mut Outputs) -> Result<()> {
    let pool =
        DescriptorPool::decode(binpb).context("decode the descriptor set (prost-reflect)")?;
    let schema = read_schema(&pool)?;
    out.insert(
        format!("{TS_GEN_DIR}/proto_hash.ts"),
        format!(
            "{TS_HEADER}\n/** Hash of all `.proto` sources; equals `flyff_proto::PROTO_HASH` (Plan/11 section 4.4). */\nexport const PROTO_HASH = \"{hash}\";\n"
        )
        .into_bytes(),
    );
    let mut s = String::from(TS_HEADER);
    wln!(s);
    wln!(
        s,
        "/** Full names of the 64-bit fields marked `(flyff.js_safe_number) = true`, sorted (Plan/11 section 4.3). */"
    );
    wln!(s, "export const JS_SAFE_FIELDS: readonly string[] = [");
    for f in &schema.js_safe {
        wln!(s, "  {f:?},");
    }
    wln!(s, "];");
    wln!(s);
    wln!(s, "const MAX_SAFE = 9007199254740991n; // 2^53 - 1");
    wln!(s);
    wln!(
        s,
        "/** bigint -> number for a js-safe field; throws a RangeError in dev builds outside +-(2^53 - 1). */"
    );
    wln!(s, "export function n64(v: bigint): number {{");
    wln!(
        s,
        "  if (import.meta.env.DEV && (v > MAX_SAFE || v < -MAX_SAFE)) {{"
    );
    wln!(
        s,
        "    throw new RangeError(`n64: ${{String(v)}} is outside +-(2^53 - 1)`);"
    );
    wln!(s, "  }}");
    wln!(s, "  return Number(v);");
    wln!(s, "}}");
    wln!(s);
    wln!(
        s,
        "/** number -> bigint for building outgoing 64-bit fields; throws a RangeError in dev builds for unsafe integers. */"
    );
    wln!(s, "export function b64(n: number): bigint {{");
    wln!(
        s,
        "  if (import.meta.env.DEV && !Number.isSafeInteger(n)) {{"
    );
    wln!(
        s,
        "    throw new RangeError(`b64: ${{String(n)}} is not a safe integer`);"
    );
    wln!(s, "  }}");
    wln!(s, "  return BigInt(n);");
    wln!(s, "}}");
    out.insert(format!("{TS_GEN_DIR}/js_safe.ts"), s.into_bytes());
    // the admin protocol lives where buf puts it (`flyff/admin/v1/admin_pb.ts`); this is the short path of Plan/25 §3.16
    out.insert(
        format!("{TS_GEN_DIR}/admin/index.ts"),
        format!(
            "{TS_HEADER}\n/** The admin console protocol (package `{ADMIN_PACKAGE}`, Plan/25 section 3.16). */\nexport * from \"../flyff/admin/v1/admin_pb\";\n"
        )
        .into_bytes(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------------
// writing / checking
// ---------------------------------------------------------------------------------------------------

fn list_generated(ctx: &Ctx, rel_dir: &str) -> Result<Vec<String>> {
    let base = ctx.path(rel_dir);
    let mut v = Vec::new();
    if !base.is_dir() {
        return Ok(v);
    }
    for entry in walkdir::WalkDir::new(&base).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(&ctx.root)
                .context("strip prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            v.push(rel);
        }
    }
    Ok(v)
}

/// Differences between the computed outputs and the files on disk (missing, changed, unexpected).
fn diff_outputs(ctx: &Ctx, out: &Outputs) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for (rel, bytes) in out {
        match std::fs::read(ctx.path(rel)) {
            Ok(disk) if disk == *bytes => {}
            Ok(_) => stale.push(format!("{rel} (content differs)")),
            Err(_) => stale.push(format!("{rel} (missing)")),
        }
    }
    for dir in [RUST_GEN_DIR, TS_GEN_DIR] {
        for rel in list_generated(ctx, dir)? {
            if !out.contains_key(&rel) {
                stale.push(format!("{rel} (no longer generated)"));
            }
        }
    }
    Ok(stale)
}

/// Writes changed files, removes unexpected files of the generated directories; returns the number of
/// files written or removed.
fn write_outputs(ctx: &Ctx, out: &Outputs) -> Result<usize> {
    let mut changed = 0;
    for (rel, bytes) in out {
        let path = ctx.path(rel);
        if std::fs::read(&path).is_ok_and(|disk| disk == *bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        changed += 1;
    }
    for dir in [RUST_GEN_DIR, TS_GEN_DIR] {
        for rel in list_generated(ctx, dir)? {
            if !out.contains_key(&rel) {
                std::fs::remove_file(ctx.path(&rel))?;
                println!("    removed stale {rel}");
                changed += 1;
            }
        }
    }
    Ok(changed)
}
