//! Raw value types shared by all formats (Plan/30 §2.1): [`RawStr`], [`Vec3`], [`Quat`].

pub use flyff_common::math::D3dMatrix;

/// `x, y, z` (left-handed, +Y up).
pub type Vec3 = [f32; 3];
/// `x, y, z, w` (`D3DXQUATERNION` order, ref/18 §3).
pub type Quat = [f32; 4];

/// A string kept as raw bytes without its terminating NUL.
///
/// Bone names, `.sfx` part names and `.dyo` editor names contain CP949 Korean (Plan/30 §0.3), so they are
/// never forced into UTF-8; callers decode them with `flyff_vfs::text::decode_text` when they need text.
#[derive(Clone, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct RawStr(pub Box<[u8]>);

impl RawStr {
    /// The bytes (without NUL).
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when the string has no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// ASCII lower-cased copy (non-ASCII bytes are unchanged).
    pub fn to_ascii_lowercase(&self) -> RawStr {
        RawStr(self.0.to_ascii_lowercase().into_boxed_slice())
    }

    /// ASCII case-insensitive comparison with `s`.
    pub fn eq_ignore_ascii_case(&self, s: &str) -> bool {
        self.0.eq_ignore_ascii_case(s.as_bytes())
    }

    /// Text view for logs and debug output: every non-ASCII byte becomes U+FFFD.
    pub fn to_string_lossy_ascii(&self) -> String {
        self.0
            .iter()
            .map(|&b| {
                if b.is_ascii() {
                    char::from(b)
                } else {
                    '\u{FFFD}'
                }
            })
            .collect()
    }
}

impl From<&[u8]> for RawStr {
    fn from(bytes: &[u8]) -> RawStr {
        RawStr(bytes.into())
    }
}

impl From<&str> for RawStr {
    fn from(s: &str) -> RawStr {
        RawStr(s.as_bytes().into())
    }
}

impl core::fmt::Debug for RawStr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.to_string_lossy_ascii())
    }
}
