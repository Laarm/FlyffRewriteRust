//! The codec of the admin console protocol (Plan/25 §3.16, §11.4; WP-ADMIN-05): one `AdminClientMsg` or
//! `AdminServerMsg` per binary WebSocket frame of `/admin/ws`, at most [`MAX_FRAME_BYTES`] long. There is no
//! envelope, no batching and no legacy id: the frame is the protobuf message.

use crate::admin_pb::{AdminClientMsg, AdminServerMsg};
use prost::Message as _;

/// The largest frame either side accepts (25 §11.4: 1 MiB).
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// Why a frame could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdminCodecError {
    /// The frame is longer than [`MAX_FRAME_BYTES`].
    #[error("the frame is longer than {MAX_FRAME_BYTES} bytes")]
    TooBig,
    /// The bytes are not a protobuf message of the protocol.
    #[error("malformed protobuf message")]
    Malformed,
}

/// Encodes a server message as one frame.
pub fn encode_server(msg: &AdminServerMsg) -> Vec<u8> {
    msg.encode_to_vec()
}

/// Encodes a client message as one frame (the console's client, tests and bots).
pub fn encode_client(msg: &AdminClientMsg) -> Vec<u8> {
    msg.encode_to_vec()
}

/// Decodes one client frame (strict: the whole frame must be a valid message).
pub fn decode_client(frame: &[u8]) -> Result<AdminClientMsg, AdminCodecError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(AdminCodecError::TooBig);
    }
    AdminClientMsg::decode(frame).map_err(|_| AdminCodecError::Malformed)
}

/// Decodes one server frame (the console's client, tests and bots).
pub fn decode_server(frame: &[u8]) -> Result<AdminServerMsg, AdminCodecError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(AdminCodecError::TooBig);
    }
    AdminServerMsg::decode(frame).map_err(|_| AdminCodecError::Malformed)
}
