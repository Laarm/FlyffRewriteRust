//! The admin persistence group (Plan/25 §11.6, WP-ADMIN-15): the tables of migration `0008_admin`, their
//! `DbOp`s, the admin atomic kinds and the chunked admin-read framework. The API is documented at the top of
//! `crates/flyff-db/NOTES.md` ("Admin console API").
//!
//! * [`model`] - the records (bookmarks, templates, notes, scheduled notices, IP blocks, TOTP, idempotency, the
//!   staff row, the offline-edit payload),
//! * [`repo`] / [`idempotency`] - the SQL behind the new `DbOp`s (writer side),
//! * [`batches`] - `AtomicBatch::admin_*` constructors,
//! * [`scan`] - [`AdminScan`], chunking, the admin reader, the heavy-scan permit, [`ScanStream`],
//! * [`reads`] - [`AdminReadQuery`] / [`AdminReadResult`] and the simple reads,
//! * `fixture` (feature `testing`) - the 10,000-character fixture generator.

pub mod batches;

pub mod idempotency;
pub mod items;
pub mod model;
pub mod players;
pub mod reads;
pub mod repo;
pub mod scan;

pub use model::{
    AdminBookmarkRecord, AdminNoteRecord, AdminOfflineEdit, AdminTemplateRecord,
    IdempotencyDecision, IdempotencyRecord, IdempotencyState, IpBlockRecord, LayerPolicy,
    NoteSubject, NoteSubjectKind, ScheduledNoticeRecord, StaffAccountRecord, TemplateKind,
    TotpRecord, normalize_cidr,
};
pub use players::{
    AccountBankRow, AccountDetailView, AccountHit, AltAccount, AltCluster, AltRow, BanRow,
    CharacterDocsView, CharacterHit, CharacterSearch, CharacterStatus, LOGIN_BLOCK_FOREVER,
};
pub use reads::{
    AdminGrantRow, AdminReadQuery, AdminReadResult, AdminRemovalRow, Page, PendingFilter,
    PendingState,
};
pub use scan::{
    AdminScan, AdminScanExt, Budget, ChunkLimits, KeysetScan, Once, Progress, ScanEvent, ScanStream,
};
