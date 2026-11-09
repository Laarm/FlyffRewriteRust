//! The messenger model (Plan/21 §9.1; ref/14 §9.1; `_Common/rtmessenger.h`): the states, the friend list of one
//! character in the world and the pending friend summon.
//!
//! The lists live in the [`MessengerService`](super::MessengerService), not on the avatar: the join contributors
//! and `player_data::related` have no avatars to reach. A friendship is mutual and stored as two rows; the block flag
//! belongs to the owner's row. The state a friend SEES is never cached (it is computed by
//! [`MessengerService::seen_state`](super::MessengerService::seen_state)): that is the deviation from the plan's
//! `FriendView.state`.

use flyff_common::CharacterId;
use std::collections::BTreeMap;

/// The longest friend list of one side (`MAX_FRIEND`, ref/14 §9.1): a list with this many entries is full.
pub const MAX_FRIEND: usize = 200;

/// `FRS_*` (`_Common/messenger.h:6-18`, `messenger.proto`): the state a character shows its friends.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FriendState {
    /// `FRS_ONLINE`: the default.
    Online = 0,
    /// `FRS_OFFLINE`: automatic (logged out, or seen through a block).
    Offline = 1,
    /// `FRS_BLOCK`: legacy.
    Block = 2,
    /// `FRS_ABSENT`: set by the user.
    Absent = 3,
    /// `FRS_HARDPLAY`: set by the user.
    HardPlay = 4,
    /// `FRS_EAT`: set by the user.
    Eat = 5,
    /// `FRS_REST`: set by the user.
    Rest = 6,
    /// `FRS_MOVE`: set by the user.
    Move = 7,
    /// `FRS_DIE`: set by the client automatically.
    Die = 8,
    /// `FRS_DANGER`: set by the client automatically.
    Danger = 9,
    /// `FRS_OFFLINEBLOCK`: legacy.
    OfflineBlock = 10,
    /// `FRS_AUTOABSENT`: set by the client when idle; relayed to the friends, never stored.
    AutoAbsent = 11,
}

impl FriendState {
    /// The highest valid state (`FRS_AUTOABSENT`).
    pub const MAX: u32 = FriendState::AutoAbsent as u32;

    /// The wire value.
    pub const fn wire(self) -> u32 {
        self as u32
    }

    /// The state of a wire value; `None` above [`FriendState::MAX`].
    pub const fn from_wire(v: u32) -> Option<FriendState> {
        Some(match v {
            0 => FriendState::Online,
            1 => FriendState::Offline,
            2 => FriendState::Block,
            3 => FriendState::Absent,
            4 => FriendState::HardPlay,
            5 => FriendState::Eat,
            6 => FriendState::Rest,
            7 => FriendState::Move,
            8 => FriendState::Die,
            9 => FriendState::Danger,
            10 => FriendState::OfflineBlock,
            11 => FriendState::AutoAbsent,
            _ => return None,
        })
    }

    /// `true` for the states a login keeps: online and the five the user picks (3..=7). Every automatic or legacy
    /// state loads as Online: a character that logged out dead must not come back "dead".
    pub const fn survives_login(self) -> bool {
        matches!(
            self,
            FriendState::Online
                | FriendState::Absent
                | FriendState::HardPlay
                | FriendState::Eat
                | FriendState::Rest
                | FriendState::Move
        )
    }
}

/// The state a character shows after a login, from the value stored in `characters.messenger_state`: the user-set
/// states (0 and 3..=7) are kept, every other value (automatic, legacy or out of range) becomes Online.
pub const fn login_state(stored: u32) -> u32 {
    match FriendState::from_wire(stored) {
        Some(s) if s.survives_login() => stored,
        _ => FriendState::Online as u32,
    }
}

/// The multi-server channel number of a friend that is in the world (`m_uIdofMulti`, one server).
pub const MULTI_ONLINE: u32 = 1;
/// The channel number of a friend that is not in the world.
pub const MULTI_OFFLINE: u32 = 100;

/// One entry of a friend list: MY block flag on this friend. (The state the friend shows is computed.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FriendView {
    /// I blocked this friend (`bBlock`).
    pub blocked: bool,
}

/// The messenger of one character in the world (`CRTMessenger`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Messenger {
    /// The state this character shows (`FRS_*`).
    pub my_state: u32,
    /// The friends in `CharacterId` order.
    pub friends: BTreeMap<CharacterId, FriendView>,
}

impl Messenger {
    /// An empty messenger showing `my_state`.
    pub fn new(my_state: u32) -> Messenger {
        Messenger {
            my_state,
            friends: BTreeMap::new(),
        }
    }

    /// `IsBlock`: `true` when `other` is blocked **and for everybody who is not a friend** (KEEP, ref/14 §9.1).
    pub fn is_blocked(&self, other: CharacterId) -> bool {
        self.friends.get(&other).is_none_or(|f| f.blocked)
    }

    /// `true` when `other` is on the list.
    pub fn is_friend(&self, other: CharacterId) -> bool {
        self.friends.contains_key(&other)
    }

    /// Number of friends.
    pub fn len(&self) -> usize {
        self.friends.len()
    }

    /// `true` without friends.
    pub fn is_empty(&self) -> bool {
        self.friends.is_empty()
    }

    /// `true` when no friend fits any more (`size() >= MAX_FRIEND`).
    pub fn is_full(&self) -> bool {
        self.friends.len() >= MAX_FRIEND
    }
}

/// A friend summon in flight (VR-027, ref/07b §18.1): the lock the original kept as `m_bQuery` on the scroll. At
/// most one per summoner; no time limit (as the original lock); dropped by the confirm, the cancel, when either side
/// leaves the world, or when the scroll is gone at the confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSummon {
    /// The player who used the scroll.
    pub summoner: CharacterId,
    /// The friend who was asked.
    pub target: CharacterId,
    /// The packed word of the request, echoed by the friend's answer (the scroll's inventory object id in the LOW
    /// word).
    pub data: u32,
    /// The inventory object id of the scroll (`LOWORD(data)`).
    pub slot: u16,
}
