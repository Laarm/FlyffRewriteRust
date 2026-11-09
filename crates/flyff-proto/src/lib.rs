//! `flyff-proto`: the wire protocol (Plan/11, 02 §5). Tier N, wasm32-compatible (not shipped to the
//! browser; the browser uses the TypeScript output of the same `.proto` files).
//!
//! * [`pb`] (= `flyff::v1`): prost messages generated from `proto/flyff/v1/*.proto`
//!   by `cargo xtask gen-proto` and committed under `src/gen/` (no build script, no `protoc`),
//! * [`PROTO_HASH`], [`LEGACY_IDS`], [`EXT_IDS`], [`CLIENT_MSG_NAMES`], [`SERVER_MSG_NAMES`] and the
//!   handler traits [`ClientHandlers`] / [`ServerHandlers`]: generated tables and dispatchers,
//! * [`frame`]: [`EncodedMsg`] / [`SessionOutbox`] (encode-once fan-out, per-tick batches split at
//!   `net.max_batch_bytes`), [`decode_client`] (strict, PL-024) and [`decode_batch`],
//! * [`limits`]: [`Limits`] generated from `(flyff.max_len)` (11 §5.7).
//!
//! `From<T>` impls for both envelope oneofs let game code write
//! `outbox.push_new(0, pb::Kick { .. })` (11 §3.4).

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]
#![warn(missing_docs)]

pub mod admin;
pub mod frame;
mod kick;
pub mod limits;

pub use frame::{
    BATCH_TAIL_MAX, EncodedMsg, ProtoError, SessionOutbox, decode_batch, decode_client,
    decode_client_msg,
};
pub use limits::{LimitViolation, Limits};

/// Generated prost code (`cargo xtask gen-proto`, committed). `flyff` holds the custom-option package
/// (`options.proto`); `flyff::v1` holds every wire message.
#[allow(
    missing_docs,
    unused_qualifications,
    clippy::all,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
pub mod flyff {
    include!("gen/flyff.rs");

    /// Package `flyff.v1`: all wire messages.
    pub mod v1 {
        include!("gen/flyff.v1.rs");
    }

    /// Packages below `flyff.admin`.
    pub mod admin {
        /// Package `flyff.admin.v1`: the admin console protocol (Plan/25 §3.16), spoken over `/admin/ws` only.
        pub mod v1 {
            include!("gen/flyff.admin.v1.rs");
        }
    }
}

/// The path every document uses: `flyff_proto::pb::Hello` (11 §3, 02 §5).
pub use flyff::v1 as pb;

/// The admin console protocol (`proto/flyff/admin/v1/admin.proto`, Plan/25 §3.16, WP-ADMIN-05): one
/// `AdminClientMsg` or `AdminServerMsg` per binary WebSocket frame of `/admin/ws`. No envelope, no legacy ids; it is
/// part of [`PROTO_HASH`] but of none of the wire tables.
pub use flyff::admin::v1 as admin_pb;

/// Decoded in-world client message: the oneof of `ClientEnvelope`, carried by `GameInput::Client`.
pub type ClientMsg = pb::client_envelope::Msg;

/// The descriptor set the generated code was built from (reflection in debug tools, `proto-coverage`).
pub static DESCRIPTOR_SET: &[u8] = include_bytes!("gen/descriptor.binpb");

include!("gen/proto_hash.rs");

/// Legacy packet/snapshot namespace of an annotated message (11 §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyNamespace {
    /// `PACKETTYPE_*`.
    Packet,
    /// `SNAPSHOTTYPE_*`.
    Snapshot,
}

/// Transport direction of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Client to server.
    ClientToServer,
    /// Server to client.
    ServerToClient,
}

/// One row of the generated legacy-id table (02 §5 step 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyIdEntry {
    /// Full protobuf message name.
    pub message: &'static str,
    /// Legacy namespace.
    pub ns: LegacyNamespace,
    /// Legacy numeric id.
    pub value: u32,
    /// Legacy constant name (original spelling, without the `PACKETTYPE_`/`SNAPSHOTTYPE_` prefix).
    pub name: &'static str,
    /// Legacy sub-id annotation (may be empty).
    pub sub: &'static str,
    /// Direction.
    pub dir: Direction,
    /// Extension id (`0xFE0000xx`) or `0`.
    pub ext_id: u32,
}

include!("gen/legacy_ids.rs");

mod from_impls {
    use crate::pb;
    include!("gen/from_impls.rs");
}

mod limit_impls {
    use crate::limits::{LimitViolation, Limits};
    use crate::pb;
    include!("gen/limits.rs");
}

mod dispatch {
    use crate::pb;
    include!("gen/dispatch.rs");
}

pub use dispatch::{
    CLIENT_MSG_NAMES, ClientCase, ClientHandlers, MsgCase, SERVER_MSG_NAMES, ServerHandlers,
    dispatch_client, dispatch_server,
};
