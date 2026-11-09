//! The effect message helpers (Plan/15 §17.3, WP-WORLD-17; ref/08 §10.1, §14): `CreateSfxObj`,
//! `RemoveSfxObj`, `PlayMusic` and `PlaySound`.
//!
//! | helper | message | `obj_id` | audience | legacy |
//! |---|---|---|---|---|
//! | [`sfx_near`] | `CreateSfxObj` | the anchor | `NearInclSelf` of the anchor | `AddCreateSfxObj` |
//! | [`sfx_world`] | `CreateSfxObj` | 0 | `World(world)` | `AddWorldCreateSfxObj` |
//! | [`sfx_remove`] | `RemoveSfxObj` | the anchor | `NearInclSelf` of the anchor | `AddRemoveSfxObj` |
//! | [`music_to`] / [`music_world`] | `PlayMusic` | 0 | one session / `World(world)` | `AddPlayMusic`, core relay |
//! | [`sound_to`] / [`sound_world`] | `PlaySound` | 0 | one session / `World(world)` | `AddPlaySound(2)` |
//!
//! `pos = (0, 0, 0)` attaches the effect to the anchor object; any other position is a world position. The
//! `flag` of the sfx messages is ignored by the client. A `PlaySound` is either a sound-table id or a wave file
//! name (`Sound::File`, at most [`MAX_SOUND_FILE`] bytes; played by the client only when its voice option is on).
//!
//! Each helper has a message builder ([`create_sfx_msg`], [`remove_sfx_msg`], [`music_msg`], [`sound_msg`]) for
//! the call sites that collect their messages in an [`Out`](crate::stats::Out) or an [`Fx`](crate::item::Fx).
//! The 87 call sites of the original (level-up, heal, upgrades, teleport flashes, GM `/PlayMusic`) belong to docs
//! 16-23.

use crate::ctx::GameCtx;
use crate::outbox::Audience;
use flyff_common::math::Vec3;
use flyff_common::{ObjId, SessionId, WorldId};
use flyff_proto::pb;

/// The longest wave file name of a `PlaySound` (`MAX_PATH`).
pub const MAX_SOUND_FILE: usize = 260;

/// What a `PlaySound` plays: the sound-table id (kind 0) or a wave file (kind 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sound {
    /// `SND_*` id of the sound table.
    Id(u32),
    /// A wave file name; cut to [`MAX_SOUND_FILE`] bytes (on a character boundary) when longer.
    File(String),
}

/// `CreateSfxObj{sfx, pos, flag}`.
pub fn create_sfx_msg(sfx: u32, pos: Vec3, flag: bool) -> pb::CreateSfxObj {
    pb::CreateSfxObj {
        sfx_id: sfx,
        x: pos.x,
        y: pos.y,
        z: pos.z,
        flag,
    }
}

/// `RemoveSfxObj{sfx, pos, flag}` (the layout of `CreateSfxObj`).
pub fn remove_sfx_msg(sfx: u32, pos: Vec3, flag: bool) -> pb::RemoveSfxObj {
    pb::RemoveSfxObj {
        sfx_id: sfx,
        x: pos.x,
        y: pos.y,
        z: pos.z,
        flag,
    }
}

/// `PlayMusic{music_id}`.
pub fn music_msg(id: u32) -> pb::PlayMusic {
    pb::PlayMusic { music_id: id }
}

/// `PlaySound` with the id or the file branch of the oneof.
pub fn sound_msg(sound: &Sound) -> pb::PlaySound {
    let sound = match sound {
        Sound::Id(id) => pb::play_sound::Sound::Id(*id),
        Sound::File(f) => pb::play_sound::Sound::File(cut_file(f)),
    };
    pb::PlaySound { sound: Some(sound) }
}

fn cut_file(f: &str) -> String {
    if f.len() <= MAX_SOUND_FILE {
        return f.to_owned();
    }
    let mut end = MAX_SOUND_FILE;
    while end > 0 && !f.is_char_boundary(end) {
        end -= 1;
    }
    f.get(..end).unwrap_or_default().to_owned()
}

/// `AddCreateSfxObj`: the effect `sfx` at `pos` (or on the anchor when `pos` is the zero vector) to everybody who
/// sees `anchor`, the anchor included when it is a player. `obj_id` of the message is `anchor`.
pub fn sfx_near(ctx: &mut GameCtx<'_>, anchor: ObjId, sfx: u32, pos: Vec3, flag: bool) {
    ctx.broadcast(
        None,
        anchor,
        Audience::NearInclSelf,
        create_sfx_msg(sfx, pos, flag),
    );
}

/// `AddWorldCreateSfxObj`: the effect to every player of `world` (any layer), `obj_id` 0.
pub fn sfx_world(ctx: &mut GameCtx<'_>, world: WorldId, sfx: u32, pos: Vec3, flag: bool) {
    ctx.broadcast(
        None,
        ObjId::NULL,
        Audience::World(world),
        create_sfx_msg(sfx, pos, flag),
    );
}

/// `AddRemoveSfxObj`: stops a looping effect for everybody who sees `anchor`.
pub fn sfx_remove(ctx: &mut GameCtx<'_>, anchor: ObjId, sfx: u32, pos: Vec3, flag: bool) {
    ctx.broadcast(
        None,
        anchor,
        Audience::NearInclSelf,
        remove_sfx_msg(sfx, pos, flag),
    );
}

/// `AddPlayMusic`: the BGM `id` to one session (`obj_id` 0). A session that is not live receives nothing.
pub fn music_to(ctx: &mut GameCtx<'_>, sid: SessionId, id: u32) {
    ctx.outbox.to_session(sid, 0, music_msg(id));
}

/// The BGM `id` to every player of `world` (the core relay of `/PlayMusic`).
pub fn music_world(ctx: &mut GameCtx<'_>, world: WorldId, id: u32) {
    ctx.broadcast(None, ObjId::NULL, Audience::World(world), music_msg(id));
}

/// `AddPlaySound(2)`: a sound to one session (`obj_id` 0).
pub fn sound_to(ctx: &mut GameCtx<'_>, sid: SessionId, sound: &Sound) {
    ctx.outbox.to_session(sid, 0, sound_msg(sound));
}

/// A sound to every player of `world`.
pub fn sound_world(ctx: &mut GameCtx<'_>, world: WorldId, sound: &Sound) {
    ctx.broadcast(None, ObjId::NULL, Audience::World(world), sound_msg(sound));
}
