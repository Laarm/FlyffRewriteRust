// The two tender messages and their window requests (`src:Neuz/DPClient.cpp:16846-16901`, `_Interface/WndWorld.cpp:4343-4489`; Plan/45 §28.5; WP-CFEAT-45). PURE builders: the module hands them the facts
// (the help panel, the guild names, the senders) and opens the window with the request.
//
//   Gc1to1TenderOpenWndNtf{ my_penya, join_penya }   `ui.siege.openOffer( { mode: 1, min: join_penya, myBid: my_penya, info: panel `guildcombatredchipguild`, bid } )`: the 675 window presets the minimum
//                                                    for a guild without a tender (`my_penya == 0`), else its own tender (the window does `SetGold`); ACCEPT confirms and sends `Gc1to1Tender{ penya }`
//   Gc1to1TenderViewNtf{ penya, ranking, remain_s, guilds }   `ui.gc1to1.openOfferState( ... )` (676, ui-lord's shell): the rows are the guild NAMES in the order sent ('' for a guild the client does
//                                                    not know: FIX, the C++ dereferenced NULL), the own rank `ranking > 0 ? ranking : -1` (blank), the own tender, no prize line; BUTTON1 asks for the bid
//                                                    window again (`Gc1to1TenderOpenWndReq`), BUTTON2 for a fresh list (`Gc1to1TenderViewReq`)

import type { OfferRequest, StateRequest } from '../../../ui/windows/siege/ports';
import type { Gc1to1Senders } from './senders';

export interface TenderOpenMsg {
  readonly myPenya: number;
  readonly joinPenya: number;
}

export interface TenderViewMsg {
  readonly penya: number;
  readonly ranking: number;
  readonly remainS: number;
  readonly guilds: readonly number[];
}

export interface TenderEnv {
  /** The panel `guildcombatredchipguild` (empty while the help bundle has none). */
  readonly info: string;
  /** The name of a guild the client knows, `null` for none. */
  guildName(id: number): string | null;
  readonly send: Pick<Gc1to1Senders, 'tender' | 'tenderOpenWnd' | 'tenderView'>;
}

/** The request of the 675 window. */
export function offerRequest(m: TenderOpenMsg, env: TenderEnv): OfferRequest {
  return {
    mode: 1,
    min: m.joinPenya,
    myBid: m.myPenya,
    info: env.info,
    bid: (penya) => {
      env.send.tender(penya);
    },
  };
}

/** The request of the 676 window. */
export function stateRequest(m: TenderViewMsg, env: TenderEnv): StateRequest {
  return {
    secondsToNext: m.remainS,
    guilds: m.guilds.map((id) => env.guildName(id) ?? ''),
    rank: m.ranking > 0 ? m.ranking : -1,
    myBid: m.penya,
    prize: 0,
    apply: () => {
      env.send.tenderOpenWnd();
    },
    refresh: () => {
      env.send.tenderView();
    },
  };
}
