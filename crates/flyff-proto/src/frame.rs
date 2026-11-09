//! Batch framing, encode-once fan-out and strict client decoding (Plan/11 §1.2, §3.4, §3.5).
//!
//! A protobuf `repeated ServerMsg msgs = 1` field is encoded as `key(1, LEN) | varint(len) | body`,
//! repeated. Concatenating those fragments and appending `key(2, VARINT) | varint(server_frame)` gives a
//! valid `ServerBatch`. A broadcast is therefore encoded **once** into an [`EncodedMsg`] and the same
//! bytes are appended to every viewer's [`SessionOutbox`] (the legacy `GETBLOCK`/`AddBlock` pattern).

use crate::pb;
use bytes::{Bytes, BytesMut};
use prost::Message as _;
use prost::encoding;
use smallvec::SmallVec;

/// Field number of `ServerBatch.msgs`.
const BATCH_MSGS_TAG: u32 = 1;
/// Field number of `ServerBatch.server_frame`.
const BATCH_FRAME_TAG: u32 = 2;
/// Upper bound of the `server_frame` tail: one key byte plus a 10 byte varint.
pub const BATCH_TAIL_MAX: usize = 11;

/// One `ServerMsg` already encoded as a complete `ServerBatch.msgs` field (key + length + body).
/// Cheap to clone (reference counted) and to append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedMsg(Bytes);

impl EncodedMsg {
    /// Encodes a prepared `ServerMsg`.
    pub fn encode(msg: &pb::ServerMsg) -> Self {
        let mut b = BytesMut::with_capacity(msg.encoded_len() + 6);
        encoding::message::encode(BATCH_MSGS_TAG, msg, &mut b);
        EncodedMsg(b.freeze())
    }

    /// Builds the `ServerMsg` from a body (the generated `From<T> for server_msg::Msg`) and encodes it.
    pub fn new(obj_id: u32, body: impl Into<pb::server_msg::Msg>) -> Self {
        Self::encode(&pb::ServerMsg {
            obj_id,
            msg: Some(body.into()),
        })
    }

    /// Encoded length in bytes (key + length prefix + body).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` for a zero-length encoding (never produced by [`EncodedMsg::encode`]).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The encoded bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Per-session outgoing buffer, owned by the game thread (one per in-world session; 11 §3.4).
#[derive(Debug, Default)]
pub struct SessionOutbox {
    buf: BytesMut,
    /// End offset (exclusive) of every entry in `buf`, ascending. Offsets saturate at `u32::MAX`
    /// (a buffer that large is cut long before by the queue limits).
    ends: SmallVec<[u32; 64]>,
    urgent: bool,
}

impl SessionOutbox {
    /// An empty outbox.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty outbox with room for `bytes` bytes.
    pub fn with_capacity(bytes: usize) -> Self {
        SessionOutbox {
            buf: BytesMut::with_capacity(bytes),
            ..Self::default()
        }
    }

    fn mark_end(&mut self) {
        self.ends
            .push(u32::try_from(self.buf.len()).unwrap_or(u32::MAX));
    }

    /// Appends an already encoded message (broadcast path).
    pub fn push(&mut self, m: &EncodedMsg) {
        self.buf.extend_from_slice(&m.0);
        self.mark_end();
    }

    /// Encodes and appends a message for this session only.
    pub fn push_new(&mut self, obj_id: u32, body: impl Into<pb::server_msg::Msg>) {
        let msg = pb::ServerMsg {
            obj_id,
            msg: Some(body.into()),
        };
        encoding::message::encode(BATCH_MSGS_TAG, &msg, &mut self.buf);
        self.mark_end();
    }

    /// Requests a flush right after the current handler (`Replace`, `Kick`, ...; 11 §3.5).
    pub fn mark_urgent(&mut self) {
        self.urgent = true;
    }

    /// `true` after [`SessionOutbox::mark_urgent`] until the next [`SessionOutbox::take_frames`].
    pub fn is_urgent(&self) -> bool {
        self.urgent
    }

    /// Number of queued messages.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// `true` when no message is queued.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Number of queued message bytes.
    pub fn queued_bytes(&self) -> usize {
        self.buf.len()
    }

    /// Takes everything queued as one or more WebSocket frames, each a valid `ServerBatch` carrying
    /// `server_frame` (written last; omitted when 0, the proto3 default). A batch is split at message
    /// boundaries so that the messages of one frame total at most `max_batch` bytes; a single message
    /// larger than that gets a frame of its own. Order is preserved. An empty outbox yields no frame.
    /// Resets the urgent flag.
    pub fn take_frames(&mut self, server_frame: u64, max_batch: usize) -> SmallVec<[Bytes; 1]> {
        self.urgent = false;
        let mut frames: SmallVec<[Bytes; 1]> = SmallVec::new();
        if self.ends.is_empty() {
            self.buf.clear();
            return frames;
        }
        let total = self.buf.len();
        if total <= max_batch {
            // Fast path: append the tail in place and hand the whole allocation over without a copy.
            write_tail(&mut self.buf, server_frame);
            self.ends.clear();
            frames.push(self.buf.split().freeze());
            return frames;
        }
        // Slow path: cut at entry boundaries, greedily.
        let mut chunk_start = 0usize;
        let mut prev_end = 0usize;
        for &end in &self.ends {
            let end = usize::try_from(end).unwrap_or(usize::MAX).min(total);
            if end - chunk_start > max_batch && prev_end > chunk_start {
                frames.push(self.cut(chunk_start, prev_end, server_frame));
                chunk_start = prev_end;
            }
            prev_end = end;
        }
        if prev_end > chunk_start {
            frames.push(self.cut(chunk_start, prev_end, server_frame));
        }
        self.buf.clear();
        self.ends.clear();
        frames
    }

    fn cut(&self, from: usize, to: usize, server_frame: u64) -> Bytes {
        let body = self.buf.get(from..to).unwrap_or_default();
        let mut frame = BytesMut::with_capacity(body.len() + BATCH_TAIL_MAX);
        frame.extend_from_slice(body);
        write_tail(&mut frame, server_frame);
        frame.freeze()
    }
}

fn write_tail(buf: &mut BytesMut, server_frame: u64) {
    if server_frame != 0 {
        encoding::uint64::encode(BATCH_FRAME_TAG, &server_frame, buf);
    }
}

/// Why a frame was rejected. Nothing is ever zero-filled or truncated (PL-024).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtoError {
    /// The frame exceeds the configured size cap (`net.max_client_msg_bytes`, 8192).
    #[error("client frame of {len} bytes exceeds the cap of {max}")]
    TooBig {
        /// Frame length.
        len: usize,
        /// Cap.
        max: usize,
    },
    /// The frame is empty.
    #[error("empty client frame")]
    Empty,
    /// The envelope decoded but carries no known message (`msg` unset, e.g. an unknown oneof tag).
    #[error("envelope carries no known message")]
    NoMessage,
    /// A varint is truncated or longer than 10 bytes.
    #[error("malformed varint")]
    BadVarint,
    /// A length prefix or a field runs past the end of the data.
    #[error("truncated message")]
    Truncated,
    /// A `string` field is not valid UTF-8.
    #[error("invalid UTF-8 in a string field")]
    InvalidUtf8,
    /// Any other wire-format violation (bad wire type, tag 0, unbalanced group, recursion limit).
    #[error("malformed protobuf: {0}")]
    Malformed(String),
}

impl ProtoError {
    /// Classifies a prost error. prost exposes only the message text; the strings are pinned by the
    /// unit tests (`pl_024_strict_decode`), so a prost upgrade that renames them fails the build gate.
    fn from_prost(e: &prost::DecodeError) -> ProtoError {
        let text = e.to_string();
        if text.contains("invalid string value") {
            ProtoError::InvalidUtf8
        } else if text.contains("invalid varint") {
            ProtoError::BadVarint
        } else if text.contains("buffer underflow") || text.contains("delimited length exceeded") {
            ProtoError::Truncated
        } else {
            ProtoError::Malformed(text)
        }
    }
}

/// Decodes one client WebSocket binary frame strictly (11 §1.2, PL-024): at most `max` bytes, not
/// empty, well-formed protobuf, valid UTF-8 in every `string`, and at least one known message set.
/// Generated length caps are checked separately with [`crate::Limits::check_limits`].
pub fn decode_client(frame: &[u8], max: usize) -> Result<pb::ClientEnvelope, ProtoError> {
    if frame.len() > max {
        return Err(ProtoError::TooBig {
            len: frame.len(),
            max,
        });
    }
    if frame.is_empty() {
        return Err(ProtoError::Empty);
    }
    let env = pb::ClientEnvelope::decode(frame).map_err(|e| ProtoError::from_prost(&e))?;
    if env.msg.is_none() {
        return Err(ProtoError::NoMessage);
    }
    Ok(env)
}

/// [`decode_client`] returning the inner message (what `GameInput::Client` carries).
pub fn decode_client_msg(frame: &[u8], max: usize) -> Result<crate::ClientMsg, ProtoError> {
    decode_client(frame, max)?.msg.ok_or(ProtoError::NoMessage)
}

/// Decodes one server `ServerBatch` frame (bots, tests, replay). Unknown messages inside the batch are
/// skipped by prost, exactly like an older client would.
pub fn decode_batch(frame: &[u8]) -> Result<pb::ServerBatch, ProtoError> {
    pb::ServerBatch::decode(frame).map_err(|e| ProtoError::from_prost(&e))
}
