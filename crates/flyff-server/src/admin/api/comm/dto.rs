//! The DTOs of the Communication area (Plan/25 §8, §11.10; WP-ADMIN-27): notices (now, scheduled, recurring) and
//! system mail. Generated into `web/src/admin/gen/dto.ts` (`declare`).
//!
//! Times are Unix seconds. `daily.at` is `HH:MM` in the **server's** time zone (`NoticeList.zone`); every other time
//! is an instant.

use crate::admin::dto::{CharacterId, ItemSpec, Recipients};
use serde::{Deserialize, Serialize};

/// How a notice looks to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum NoticeStyleDto {
    /// The large `/sys` line (at most 511 bytes).
    System,
    /// The centred caption (at most 260 bytes).
    Caption,
}

/// Who receives a notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoticeTargetDto {
    /// Everybody in the world.
    All,
    /// The players of one world.
    World {
        /// The world id.
        world: u32,
    },
    /// These characters (1..=100; the ones that are not online are skipped).
    Characters {
        /// The character ids.
        characters: Vec<CharacterId>,
    },
}

/// When a scheduled notice repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NoticeRepeatDto {
    /// Once, at the start minute.
    #[default]
    Once,
    /// Every this many minutes after the start (1..=43,200).
    Every {
        /// The interval.
        minutes: u32,
    },
    /// Every day at this server-local time.
    Daily {
        /// `HH:MM`.
        at: String,
    },
}

/// `POST /admin/api/broadcast` (`Mutation<BroadcastBody>`): a notice that goes out now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BroadcastBody {
    /// The text (system: at most 511 bytes, caption: at most 260).
    pub text: String,
    /// The style.
    pub style: NoticeStyleDto,
    /// The audience.
    pub target: NoticeTargetDto,
}

/// What a broadcast reached.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BroadcastResult {
    /// The players that got it.
    pub delivered: u32,
    /// For a list of characters: how many were named.
    pub requested: Option<u32>,
}

/// `POST /admin/api/notices` and `PUT /admin/api/notices/{id}` (`Mutation<NoticeBody>`): a scheduled or recurring
/// notice. An update replaces every field below (the author and the creation time stay).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NoticeBody {
    /// The text.
    pub text: String,
    /// The style.
    pub style: NoticeStyleDto,
    /// The audience.
    pub target: NoticeTargetDto,
    /// The first minute.
    pub start_at: i64,
    /// When it repeats; left out = once.
    #[serde(default)]
    pub repeat: NoticeRepeatDto,
    /// The last moment it may fire.
    #[serde(default)]
    #[cfg_attr(feature = "ts", ts(optional = nullable))]
    pub end_at: Option<i64>,
    /// Paused notices never fire.
    #[serde(default)]
    pub paused: bool,
}

/// One row of the list of scheduled notices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NoticeRow {
    /// The id.
    pub id: i64,
    /// The text.
    pub text: String,
    /// The style.
    pub style: NoticeStyleDto,
    /// The audience.
    pub target: NoticeTargetDto,
    /// The first minute.
    pub start_at: i64,
    /// When it repeats.
    pub repeat: NoticeRepeatDto,
    /// The last moment it may fire.
    pub end_at: Option<i64>,
    /// An operator paused it.
    pub paused: bool,
    /// Who made it (`staff:joe`, `owner:local`).
    pub created_by: String,
    /// When it was made.
    pub created_at: i64,
    /// The minute it fired last.
    pub last_fired_at: Option<i64>,
    /// `scheduled`, `paused`, `done`, `missed` or `ended`.
    pub state: String,
    /// The next minute it fires (`scheduled` only).
    pub next_fire: Option<i64>,
}

/// `GET /admin/api/notices`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct NoticeList {
    /// The server's clock when the list was made.
    pub now: i64,
    /// The server's time zone (`Europe/Berlin`): the zone of `daily.at`.
    pub zone: String,
    /// The offset of that zone from UTC now, seconds east.
    pub utc_offset_s: i32,
    /// The longest system and caption text, bytes.
    pub max_system_bytes: u32,
    /// See above.
    pub max_caption_bytes: u32,
    /// The most notices the schedule holds.
    pub max_notices: u32,
    /// The notices in id order.
    pub items: Vec<NoticeRow>,
}

/// `POST /admin/api/mail` (`Mutation<MailBody>`, ✱): a system mail with attachments to some recipients; the sender
/// is the system. One mail per item line, the penya on the first (a mail holds one item).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MailBody {
    /// Who receives it.
    pub recipients: Recipients,
    /// The subject (at most 31 bytes).
    pub subject: String,
    /// The text (at most 255 bytes).
    #[serde(default)]
    pub text: String,
    /// The attached items (at most 50 lines).
    #[serde(default)]
    pub items: Vec<ItemSpec>,
    /// The attached penya.
    #[serde(default)]
    pub gold: u64,
}

/// One row of the history of system mails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemMailDto {
    /// The mail id.
    pub id: u32,
    /// The receiver.
    pub receiver: CharacterId,
    /// The receiver's name.
    pub receiver_name: String,
    /// The subject.
    pub title: String,
    /// The text.
    pub text: String,
    /// The penya still attached.
    pub gold: i64,
    /// The item still attached (0 = none).
    pub item_id: u32,
    /// The pieces of it.
    pub item_count: u32,
    /// When it was sent.
    pub created_at: i64,
    /// The receiver read it.
    pub read: bool,
    /// The receiver is in the world now.
    pub online: bool,
}

/// `GET /admin/api/mail`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SystemMailList {
    /// Subject and text limits, bytes (the game cuts longer texts).
    pub max_subject_bytes: u32,
    /// See above.
    pub max_text_bytes: u32,
    /// The newest system mails first.
    pub items: Vec<SystemMailDto>,
}

/// Appends the declarations of this module to `out` (the generator of `dto.rs` calls it).
#[cfg(feature = "ts")]
pub fn declare(out: &mut String, cfg: &ts_rs::Config) {
    use ts_rs::TS;

    fn push<T: TS + 'static>(out: &mut String, cfg: &ts_rs::Config) {
        out.push_str("export ");
        out.push_str(&T::decl(cfg));
        out.push('\n');
    }

    push::<NoticeStyleDto>(out, cfg);
    push::<NoticeTargetDto>(out, cfg);
    push::<NoticeRepeatDto>(out, cfg);
    push::<BroadcastBody>(out, cfg);
    push::<BroadcastResult>(out, cfg);
    push::<NoticeBody>(out, cfg);
    push::<NoticeRow>(out, cfg);
    push::<NoticeList>(out, cfg);
    push::<MailBody>(out, cfg);
    push::<SystemMailDto>(out, cfg);
    push::<SystemMailList>(out, cfg);
}
