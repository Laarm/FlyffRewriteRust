//! `econ`: the economy of doc 19 that talks to NPCs (Plan/19 §7, WP-ECON-01, WP-ECON-02): the NPC shops
//! (penya and Red Chip vendors), quick sell and the beauty shop. Player trade, the personal vendor, the bank and
//! the mail are the work packages ECON-03 to ECON-06 and extend [`EconState`] and this module.
//!
//! | module | content |
//! |---|---|
//! | [`shop`] | [`shop::ShopStock`] (the four 100-slot tabs of a vendor, infinite), `OpenShopWndReq`, `CloseShopWnd`, `BuyItem`, `BuyChipItem`, `SellItem` |
//! | [`quick_sell`] | `QuickSell` (custom, `features.quick_sell`) |
//! | [`beauty`] | `SetHairReq`, `ChangeFaceReq` |
//! | [`tax`] | the [`tax::TaxHooks`] seam the shops read; the real implementation is `event::tax::TaxService` (`Services.event.tax`, WP-ECON-08), [`tax::NoTax`] the neutral one |
//! | [`trade`] | player trade with an atomic commit (WP-ECON-03): requests, sessions, offers, the commit, the cancellation triggers |
//! | [`vendor`] | the personal vendor (WP-ECON-04): registration, title, browsing, the atomic purchase, the close paths, the [`vendor::VendorRoomHooks`] seam |
//! | [`room`] | the vendor chat rooms (WP-ECON-11), also `chat::vendor_room` |
//! | [`lifecycle`] | the per-tick pass that ends trades and stalls, the logout cleanup ([`lifecycle::EconLogout`]) |
//! | [`grants`] | the pending grants and removals at the player (WP-ECON-09): the poll's and the enter-world pass's delivery, `apply_removals`, the join stash |
//!
//! The pure price rules (`get_cost`, `buy_unit_price`, `sell_unit_price`, `quick_sell_unit_price`,
//! `chip_cost`, `hair_cost`) are `flyff_rules::item::price`; the browser shows the same numbers through the
//! wasm `RulesContext`.

pub mod bank;
pub mod beauty;
pub mod exchange;
pub mod grants;
pub mod lifecycle;
pub mod mail;
pub mod quick_sell;
pub mod room;
pub mod shop;
pub mod tax;
pub mod trade;
pub mod vendor;

use flyff_common::ObjId;
use flyff_common::det::DetMap;
use flyff_data::GameData;
use flyff_data::character::NpcDef;
use shop::ShopStock;
use std::sync::Arc;

/// The single trade-partner slot of a player, the legacy `m_vtInfo.GetOther()` shared by the NPC shop, the
/// player trade and the browsing of a personal shop (Plan/19 §7.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TradePartner {
    /// No partner.
    #[default]
    None,
    /// The shop window of the NPC is open.
    Shop {
        /// The vendor NPC.
        npc: ObjId,
    },
    /// A player trade is running (ECON-03).
    Player {
        /// The other player.
        other: ObjId,
    },
    /// The player browses the personal shop of another player (ECON-04).
    VendorBrowse {
        /// The vending player.
        vendor: ObjId,
    },
}

/// The runtime-only economy state of a player (never persisted; Plan/19 §20.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EconState {
    /// `m_vtInfo.GetOther()`.
    pub partner: TradePartner,
    /// `m_bBank`: the bank password was confirmed (kept in step with [`EconState::bank`] by the bank
    /// handlers, ECON-05).
    pub bank_open: bool,
    /// The bank session opened by `OpenBankWnd` (Plan/19 §10.1); `bank_open` is its `pin_confirmed`.
    pub bank: Option<bank::BankSession>,
    /// The game time of the last accepted `QueryPostMail` (VR-029).
    pub mail_last_send: Option<flyff_common::GameMs>,
    /// The game time of the last accepted `QueryMailBoxReq` (VR-029).
    pub mail_last_query: Option<flyff_common::GameMs>,
    /// `m_vtInfo.VendorIsVendor()`: the player has a personal shop open (ECON-04).
    pub vending: bool,
    /// The personal shop of the player: title, registered slots (Plan/19 §9.1, WP-ECON-04). `vending` is kept in
    /// step with it by the vendor handlers (`title` set or a slot registered).
    pub vendor: vendor::VendorState,
    /// A teleport, a world change or a death ended the player's trade, stall or stall visit: the economy pass of
    /// the tick ([`lifecycle::process`]) runs the cancellation and clears the flag (the world phase that sets it
    /// has no `Services`).
    pub interrupted: bool,
}

impl EconState {
    /// `true` while any partner is set (`m_vtInfo.GetOther() != NULL`).
    pub fn has_partner(&self) -> bool {
        self.partner != TradePartner::None
    }

    /// The NPC whose shop window is open.
    pub fn shop_npc(&self) -> Option<ObjId> {
        match self.partner {
            TradePartner::Shop { npc } => Some(npc),
            _ => None,
        }
    }

    /// Teleport, death and logout end the shop session (Plan/19 §7.2). Other partners (a trade, a stall) end
    /// through their own cancellation (ECON-03/04): they are only **flagged** here (`interrupted`), because the
    /// callers (the death, the `_replace` of the world phase) have no `Services`; [`lifecycle::process`] cancels
    /// the trade, closes the stall or leaves the visited stall in the same tick.
    pub fn end_shop_session(&mut self) {
        // the same three triggers (teleport, world change, death) drop the bank session (Plan/19 §10.2)
        self.end_bank_session();
        if matches!(self.partner, TradePartner::Shop { .. }) {
            self.partner = TradePartner::None;
        }
        if self.vending
            || matches!(
                self.partner,
                TradePartner::Player { .. } | TradePartner::VendorBrowse { .. }
            )
        {
            self.interrupted = true;
        }
    }

    /// Teleport, world change, death and logout drop the bank session (Plan/19 §10.2, as `CloseBankWnd`).
    pub fn end_bank_session(&mut self) {
        self.bank = None;
        self.bank_open = false;
    }
}

/// The economy services (`Services.econ`): the built shop stocks, the bank, the mail, the exchange scripts, the trades
/// and the personal vendors. The tax is `Services.event.tax` (doc 22 `TaxService`); the shops read it from there.
#[derive(Debug, Default)]
pub struct EconService {
    /// The stock of every vendor NPC definition used so far, by lower-cased character key. Built from the
    /// loaded data on first use (the stock depends on nothing else) and never changed afterwards.
    stocks: DetMap<String, Arc<ShopStock>>,
    /// The bank: the PIN guards and the PIN comparison (Plan/19 §10, WP-ECON-05).
    pub bank: bank::BankService,
    /// The mail service: every undeleted mail, the boxes, the id counter (Plan/19 §11, WP-ECON-06).
    pub mail: mail::MailService,
    /// The exchange scripts: the campus-point seam (Plan/19 §16, WP-ECON-07).
    pub exchange: exchange::ExchangeService,
    /// Player trade: the pending requests and the running sessions (Plan/19 §8, WP-ECON-03).
    pub trade: trade::TradeService,
    /// The personal vendors and the vendor chat rooms (Plan/19 §9, WP-ECON-04, WP-ECON-11).
    pub vendor: vendor::VendorService,
}

impl EconService {
    /// The service of a starting game: the mail service loads every undeleted mail and the id counter of the
    /// startup state (13 §7, §16.9).
    pub fn from_startup(startup: &flyff_db::StartupState) -> EconService {
        EconService {
            mail: mail::MailService::from_startup(startup),
            ..EconService::default()
        }
    }

    /// The stock of `def` (built once; the stock is infinite and shared by all players).
    pub fn stock_of(&mut self, def: &NpcDef, data: &GameData) -> Arc<ShopStock> {
        let key = def.key.to_ascii_lowercase();
        if let Some(s) = self.stocks.get(&key) {
            return Arc::clone(s);
        }
        let s = Arc::new(ShopStock::build(def, data));
        self.stocks.insert(key, Arc::clone(&s));
        s
    }
}
