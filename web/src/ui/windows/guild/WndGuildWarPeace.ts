// `WndGuildWarPeace` (`CWndGuildWarPeace`, `src:_Interface/WndGuildWarPeace.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-48): `APP_GUILD_WARPEACE` (288 x 128), "Do you propose a truce
// to the opposing Guild?". Yes -> `SendQueryTruce` (the master), No closes.

import { APP_GUILD_WARPEACE } from '../../../gen/defs/resdata';
import { WndWarYesNo } from './warBase';

export class WndGuildWarPeace extends WndWarYesNo {
  readonly appId = APP_GUILD_WARPEACE;
}
