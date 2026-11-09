//! `ticket.inc` and `layer.inc`: the entrance passes and the expandable layers (`CTicketProperty::LoadScript`,
//! `CLayerProperty::LoadScript`, Plan/14 §6.13, ref/13 §21; src:_Common/ticket.cpp:36-95).
//!
//! ```text
//! ticket.inc   ITEM WORLD X Y Z          one line per ticket item
//! layer.inc    WORLD NEXPAND             one line per world
//! ```
//!
//! This data set has 5 tickets (Kebaras 3, Cisland 2, the same position per world) and 2 layer lines (both 1).
//! A repeated ticket item keeps the first line (`map::insert`; the original `ASSERT`s). `layer.inc` is a
//! `vector`, and `GetExpanedLayer` returns the first line of the world.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One ticket (`TicketProp`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TicketProp {
    /// The world the ticket opens (`WI_*`).
    pub world: u32,
    /// The arrival position.
    pub pos: [f32; 3],
}

/// The ticket and layer tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TicketTables {
    /// Ticket item to world and position.
    pub tickets: BTreeMap<u32, TicketProp>,
    /// `layer.inc` in file order: `(world, expand)`.
    pub layers: Vec<(u32, i32)>,
}

impl TicketTables {
    /// `GetTicketProp( item )`.
    pub fn ticket(&self, item: u32) -> Option<&TicketProp> {
        self.tickets.get(&item)
    }

    /// `IsTarget( world )`: some ticket opens that world.
    pub fn is_target(&self, world: u32) -> bool {
        self.tickets.values().any(|t| t.world == world)
    }

    /// `GetExpanedLayer( world )`: the first `layer.inc` line of the world, 0 without one.
    pub fn expanded_layer(&self, world: u32) -> i32 {
        self.layers
            .iter()
            .find(|(w, _)| *w == world)
            .map_or(0, |(_, n)| *n)
    }
}

#[cfg(feature = "load")]
pub(crate) use parse::{parse_layers, parse_tickets};

#[cfg(feature = "load")]
mod parse {
    use super::{TicketProp, TicketTables};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::collections::btree_map::Entry;
    use std::sync::Arc;

    /// `CTicketProperty::LoadScript` over decoded `text`; fills `out.tickets`.
    pub(crate) fn parse_tickets(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        out: &mut TicketTables,
    ) {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut item = sc.get_number();
        while !sc.finished() {
            let world = sc.get_number();
            let pos = [sc.get_float(), sc.get_float(), sc.get_float()];
            if let Entry::Vacant(e) = out.tickets.entry(item) {
                e.insert(TicketProp { world, pos });
            } else {
                sc.report(
                    Severity::Warning,
                    DiagCode::PropIdDuplicate,
                    format!("ticket.inc: ticket item {item} is listed twice; the first line wins"),
                );
            }
            item = sc.get_number();
        }
    }

    /// `CLayerProperty::LoadScript` over decoded `text`; fills `out.layers`.
    pub(crate) fn parse_layers(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
        out: &mut TicketTables,
    ) {
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        let mut world = sc.get_number();
        while !sc.finished() {
            let expand = sc.get_number_i32();
            out.layers.push((world, expand));
            world = sc.get_number();
        }
    }
}
