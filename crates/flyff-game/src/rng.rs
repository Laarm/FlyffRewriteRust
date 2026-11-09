//! `RngSet`: every random stream of the game thread, derived from `GameInit.seed` (Plan/10 §4.8, R16,
//! PL-013). The derivation helpers (`splitmix64`, `stream_tag`, `world_stream`, `named_stream`) live in
//! `flyff_common::rng`.
//!
//! Stream catalogue (a stream is never used by another owner, so one world's results do not depend on how
//! many numbers another world, service or Lua state drew):
//!
//! | stream | derivation | used by |
//! |---|---|---|
//! | world `w` | `Pcg32Rng::new(splitmix64(seed ^ w), stream = w)` | everything simulated in world `w`, all layers |
//! | `service` | `named_stream(seed, "service")` | global services without a world |
//! | `event` | `named_stream(seed, "event")` | doc 22 services |
//! | `lua_*` | `derive(stream_tag(name))` | one per Lua state (`flyff-lua`, doc 20) |
//! | legacy | `LegacyLcg::new(seed as u32)` | only KQ rows that depend on the legacy `xRand` sequence |

use flyff_common::WorldId;
use flyff_common::rng::{LegacyLcg, Pcg32Rng, derive_seed, named_stream, world_stream};
use std::collections::BTreeMap;

/// The named service streams created at `Game` construction.
pub const NAMED_STREAMS: [&str; 2] = ["service", "event"];

/// Per-Lua-state stream names (`flyff-lua` seeds its states from [`RngSet::derive`]).
pub const LUA_STREAMS: [&str; 6] = [
    "lua_event",
    "lua_tax",
    "lua_secret_room",
    "lua_rainbow_race",
    "lua_guild_combat_1to1",
    "lua_npc",
];

/// All random streams of the game thread.
#[derive(Clone, Debug)]
pub struct RngSet {
    seed: u64,
    worlds: BTreeMap<u32, Pcg32Rng>,
    named: BTreeMap<&'static str, Pcg32Rng>,
    legacy: LegacyLcg,
}

impl RngSet {
    /// Builds the streams of `world_ids` and the named service streams from `seed`.
    pub fn new(seed: u64, world_ids: impl IntoIterator<Item = WorldId>) -> RngSet {
        let mut set = RngSet {
            seed,
            worlds: BTreeMap::new(),
            named: BTreeMap::new(),
            // `xSRand(seed as DWORD)`: the low 32 bits.
            legacy: LegacyLcg::new(seed as u32),
        };
        for name in NAMED_STREAMS {
            set.named.insert(name, named_stream(seed, name));
        }
        for w in world_ids {
            set.worlds.insert(w.0, world_stream(seed, w.0));
        }
        set
    }

    /// The game seed (recorded in the replay header).
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The stream of world `w`, shared by all its layers. A world that was not announced at construction
    /// gets its stream on first use; the stream depends only on `(seed, w)`, never on creation order.
    pub fn world(&mut self, w: WorldId) -> &mut Pcg32Rng {
        let seed = self.seed;
        self.worlds
            .entry(w.0)
            .or_insert_with(|| world_stream(seed, w.0))
    }

    /// A named service stream of the catalogue (`"service"`, `"event"`).
    ///
    /// A name outside the catalogue trips a debug assertion; in release builds it is derived on first
    /// use like any other stream (still deterministic, never shared with another owner).
    pub fn named(&mut self, name: &'static str) -> &mut Pcg32Rng {
        debug_assert!(
            NAMED_STREAMS.contains(&name),
            "random stream '{name}' is not in the Plan/10 §4.8 catalogue"
        );
        let seed = self.seed;
        self.named
            .entry(name)
            .or_insert_with(|| named_stream(seed, name))
    }

    /// The `service` stream: global services without a world, mail, GM tools, the runtime.
    pub fn service(&mut self) -> &mut Pcg32Rng {
        self.named("service")
    }

    /// The `event` stream: doc 22 services.
    pub fn event(&mut self) -> &mut Pcg32Rng {
        self.named("event")
    }

    /// `splitmix64(seed ^ tag)`: the seed of a per-Lua-state stream (`flyff-lua` builds
    /// `Pcg32Rng::new(derive(tag), tag)` for `tag = stream_tag("lua_npc")`, …).
    pub fn derive(&self, tag: u64) -> u64 {
        derive_seed(self.seed, tag)
    }

    /// The legacy `xRand` generator, only where a KQ row depends on its sequence.
    pub fn legacy(&mut self) -> &mut LegacyLcg {
        &mut self.legacy
    }
}
