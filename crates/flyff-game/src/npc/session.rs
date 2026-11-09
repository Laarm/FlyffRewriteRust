//! `DialogSession` (Plan/20 §7.1, §7.4, VR-060): what the client can click.
//!
//! The legacy server trusted any `(key, param, quest)` a client sent (ref/16 §5.3: free `InitStat` at Juria,
//! quest completion at any NPC). The rewrite keeps, per player, the buttons it offered in the last responses and
//! accepts only a request that equals one of them:
//!
//! | `DialogOp` | session effect |
//! |---|---|
//! | `add_key` | an [`OfferKind::Key`] unless one with the same **word** exists (the client dedupes by word) |
//! | `new_quest` with key `QUEST_BEGIN` | [`OfferKind::NewQuest`] |
//! | `new_quest` with key `QUEST_NEXT_LEVEL` | [`OfferKind::NextLevelQuest`]: shown, **never accepted** |
//! | `curr_quest` | [`OfferKind::CurrQuest`] |
//! | `remove_key` | removes the `Key` offers with that key |
//! | `remove_all_key` | removes every `Key`, `NewQuest`, `NextLevelQuest` and `CurrQuest` offer |
//! | `add_answer` | an [`OfferKind::Answer`] in the transient list (valid for one response) |
//! | `say` | the `[Display][Key]` links of the text as [`OfferKind::Link`] in the transient list |
//! | `exit` | the session ends after the response |
//!
//! Persistent offers (keys, quest entries) live until the dialog ends; transient ones (answers, links) are cleared
//! before every accepted request, as the client's `BeginText` clears them.

use flyff_common::{GameMs, ObjId};
use flyff_proto::pb;
use flyff_proto::pb::dialog_op::Op;

/// An upper bound for the offers of one list, so a runaway script cannot grow a session without limit.
pub const MAX_OFFERS: usize = 128;

/// What kind of button an [`Offer`] is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OfferKind {
    /// A key button (`add_key`).
    Key,
    /// A quest of the "new quest" list (`QUEST_BEGIN`).
    NewQuest,
    /// A quest of the "new quest" list that is displayed red and never accepted (`QUEST_NEXT_LEVEL`).
    NextLevelQuest,
    /// A quest of the "current quest" list (`QUEST_END`).
    CurrQuest,
    /// An answer button (`add_answer`), valid for one response.
    Answer,
    /// A context link of a text, valid for one response.
    Link,
}

/// One button the client was offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// The kind.
    pub kind: OfferKind,
    /// The text of the button.
    pub word: String,
    /// The key it sends back.
    pub key: String,
    /// The `param` it sends back.
    pub param: u32,
    /// The `quest_id` it sends back.
    pub quest_id: u32,
}

/// The dialog of one player with one NPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogSession {
    /// The NPC this window belongs to.
    pub npc: ObjId,
    /// Its character key (lower case).
    pub character_key: String,
    /// `DialogOps.session_seq` of the last response.
    pub seq: u32,
    /// Persistent offers: key buttons and the quest lists.
    pub keys: Vec<Offer>,
    /// Answers and links of the last response.
    pub transient: Vec<Offer>,
}

impl DialogSession {
    /// A fresh session (`#init`).
    pub fn new(npc: ObjId, character_key: &str) -> DialogSession {
        DialogSession {
            npc,
            character_key: character_key.to_owned(),
            seq: 0,
            keys: Vec::new(),
            transient: Vec::new(),
        }
    }

    /// `true` when the client may send `(key, param, quest_id)`: it equals an offered button that can be clicked.
    pub fn accepts(&self, key: &str, param: u32, quest_id: u32) -> bool {
        self.keys.iter().chain(self.transient.iter()).any(|o| {
            o.kind != OfferKind::NextLevelQuest
                && o.key == key
                && o.param == param
                && o.quest_id == quest_id
        })
    }

    /// Applies the operations of one response (§7.4) in order. Returns `true` when one of them was `exit`.
    pub fn apply_all(&mut self, ops: &[pb::DialogOp]) -> bool {
        let mut exit = false;
        for op in ops {
            exit |= self.apply(op);
        }
        exit
    }

    /// Applies one operation; `true` for `exit`.
    pub fn apply(&mut self, op: &pb::DialogOp) -> bool {
        let Some(op) = op.op.as_ref() else {
            return false;
        };
        match op {
            Op::AddKey(k) => {
                let dup = self
                    .keys
                    .iter()
                    .any(|o| o.kind == OfferKind::Key && o.word == k.word);
                if !dup && self.keys.len() < MAX_OFFERS {
                    self.keys.push(offer(OfferKind::Key, k));
                }
            }
            Op::NewQuest(k) => {
                let kind = match k.key.as_str() {
                    "QUEST_BEGIN" => OfferKind::NewQuest,
                    "QUEST_NEXT_LEVEL" => OfferKind::NextLevelQuest,
                    // the original client treated any other key as a begin; it is never accepted here
                    _ => OfferKind::NextLevelQuest,
                };
                if self.keys.len() < MAX_OFFERS {
                    self.keys.push(offer(kind, k));
                }
            }
            Op::CurrQuest(k) => {
                if self.keys.len() < MAX_OFFERS {
                    self.keys.push(offer(OfferKind::CurrQuest, k));
                }
            }
            Op::RemoveKey(t) => {
                self.keys
                    .retain(|o| !(o.kind == OfferKind::Key && o.key == t.text));
            }
            Op::RemoveAllKey(_) => {
                self.keys.clear();
            }
            Op::AddAnswer(k) => {
                if self.transient.len() < MAX_OFFERS {
                    self.transient.push(offer(OfferKind::Answer, k));
                }
            }
            Op::Say(t) | Op::SayQuest(t) => {
                for (word, key) in parse_links(&t.text) {
                    if self.transient.len() >= MAX_OFFERS {
                        break;
                    }
                    self.transient.push(Offer {
                        kind: OfferKind::Link,
                        word,
                        key,
                        param: 0,
                        quest_id: 0,
                    });
                }
            }
            Op::Exit(_) => return true,
            _ => {}
        }
        false
    }
}

fn offer(kind: OfferKind, k: &pb::DialogKey) -> Offer {
    Offer {
        kind,
        word: k.word.clone(),
        key: k.key.clone(),
        param: k.param,
        quest_id: k.quest_id,
    }
}

/// The `[Display][Key]` links of a text (ref/16 §5.9, `CWndDialog::ParsingString`): `[` opens the display word,
/// the next `]` closes it, the key follows in a second bracket pair; an empty second pair means key = display
/// word. Anything but `[` right after the first `]` makes the link malformed and it is ignored.
pub fn parse_links(text: &str) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = rest.get(open + 1..).unwrap_or("");
        let Some(close) = after.find(']') else {
            break;
        };
        let word = after.get(..close).unwrap_or("");
        let tail = after.get(close + 1..).unwrap_or("");
        let Some(key_part) = tail.strip_prefix('[') else {
            // malformed: text between `]` and `[`
            rest = tail;
            continue;
        };
        let Some(key_end) = key_part.find(']') else {
            break;
        };
        let key = key_part.get(..key_end).unwrap_or("");
        let key = if key.is_empty() { word } else { key };
        if !word.is_empty() {
            links.push((word.to_owned(), key.to_owned()));
        }
        rest = key_part.get(key_end + 1..).unwrap_or("");
    }
    links
}

/// The dialog fields of a player (Plan/20 §7.1; the struct `PlayerExt` is doc 16's, the fields are this
/// document's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerNpc {
    /// The open dialog, if any.
    pub dialog: Option<DialogSession>,
    /// The time of the last `ScriptDlg` / `RemoveQuestReq` (VR-061): `None` before the first request, so that the
    /// first request of a new game clock is not throttled.
    pub script_req_at: Option<GameMs>,
}
