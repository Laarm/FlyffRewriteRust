// `WndGuildWarGiveUp` (`CWndGuildWarGiveUp`, `src:_Interface/WndGuildWarGiveUp.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-48): `APP_GUILD_WARGIVEUP` (288 x 128), "Do you give up
// the Guild Duel?". Yes -> `SendSurrender` (every member may), No closes.

import { APP_GUILD_WARGIVEUP } from '../../../gen/defs/resdata';
import { WndWarYesNo } from './warBase';

export class WndGuildWarGiveUp extends WndWarYesNo {
  readonly appId = APP_GUILD_WARGIVEUP;
}
