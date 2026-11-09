// `WndGuildWarPeaceConfirm` (`CWndGuildWarPeaceConfirm`, `src:_Interface/WndGuildWarPeaceConfirm.cpp`, ref/22 §17 J, Plan/42 §19.4 J, 45 §23; WP-UI-48): `APP_GUILD_WARPEACECONFIRM`
// (288 x 128), "The opposing guild has proposed a truce. Do you approve the truce?" (opened by `QueryTruceNtf`). Yes -> `SendAcptTruce`, No closes.

import { APP_GUILD_WARPEACECONFIRM } from '../../../gen/defs/resdata';
import { WndWarYesNo } from './warBase';

export class WndGuildWarPeaceConfirm extends WndWarYesNo {
  readonly appId = APP_GUILD_WARPEACECONFIRM;
}
