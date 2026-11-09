// `WndLordPledge` (`CWndLordPledge`, `src:_Interface/WndLord.cpp:184-314`, ref/22 §17 O, Plan/42 §19.4 O, 45 §28.2; WP-UI-55): `APP_LORD_PLEDGE` (720, 304 x 272, "Pledge"), the pledge of the candidate of one rank.
// `TEXT1` shows the pledge; the edit (`EDIT1`) is hidden until the candidate himself double clicks the text (only the own pledge can be edited): the edit then shows the text, `first` records whether
// the text was empty. `BUTTON2` sends: a text over 255 bytes opens the box `TID_GUILD_NOTICE_ERROR` (the original counted MBCS bytes; the server cuts at 255 UTF-8 bytes); a first pledge is sent when it is
// not empty and the window closes (also when nothing was edited); a changed pledge asks the confirm window (738) first, because it costs 1 000 000 penya. The candidate of the window is the one of the
// rank at the moment it opened (the original kept a pointer to him).

import { APP_LORD_PLEDGE, WIDC_BUTTON2, WIDC_EDIT1, WIDC_TEXT1 } from '../../../gen/defs/resdata';
import { TID_GUILD_NOTICE_ERROR } from '../../../gen/defs/text';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import type { Shortcut } from '../../dnd/shortcut';
import {
  EBS_AUTOVSCROLL,
  EBS_MULTILINE,
  EBS_WANTRETURN,
  WM_LBUTTONDBLCLK,
  Wnm,
} from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import type { LordWindowDeps, LordWindowOps } from './ports';
import { pledgeTooLong } from './rows';

export class WndLordPledge extends WndNeuz {
  readonly appId = APP_LORD_PLEDGE;
  readonly #deps: LordWindowDeps;
  readonly #ops: LordWindowOps;
  readonly #rank: number;
  /** The candidate of the rank when the window opened (`m_pRanker`); 0 = no such rank. */
  #candidate = 0;
  /** `m_bIsFirst`. */
  #first = true;

  constructor(rank: number, deps: LordWindowDeps, ops: LordWindowOps) {
    super();
    this.#rank = rank;
    this.#deps = deps;
    this.#ops = ops;
    this.putRegInfo = false;
  }

  get rank(): number {
    return this.#rank;
  }
  /** The character id of the candidate whose pledge this is. */
  get candidateId(): number {
    return this.#candidate;
  }
  /** `m_bIsFirst`. */
  get isFirst(): boolean {
    return this.#first;
  }
  get textCtrl(): TextCtrl | null {
    return this.getDlgItem<TextCtrl>(WIDC_TEXT1);
  }
  get edit(): EditCtrl | null {
    return this.getDlgItem<EditCtrl>(WIDC_EDIT1);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    const view = this.#deps.ports.view.get();
    const ranker = view.candidates[this.#rank - 1];
    this.#candidate = ranker?.id ?? 0;
    const edit = this.edit;
    if (edit) {
      edit.addWndStyle(EBS_MULTILINE | EBS_AUTOVSCROLL | EBS_WANTRETURN);
      // the editor stays hidden until the candidate asks for it
      edit.setVisible(false);
    }
    if (ranker && ranker.pledge !== '') this.textCtrl?.setString(ranker.pledge);
    this.moveParentCenter();
  }

  /** `OnCommand( WIDC_TEXT1, WM_LBUTTONDBLCLK )`: only the candidate himself edits his pledge. */
  override onCommand(id: number, msg: number, src?: WndBase): boolean {
    if (
      msg === WM_LBUTTONDBLCLK &&
      id === WIDC_TEXT1 &&
      this.#candidate !== 0 &&
      this.#deps.ports.view.get().selfId === this.#candidate
    ) {
      this.startEdit();
      return true;
    }
    return super.onCommand(id, msg, src);
  }

  /** The editor opens with the text of the pledge (`m_bIsFirst` = the text was empty). */
  startEdit(): void {
    const text = this.textCtrl;
    const edit = this.edit;
    if (!text || !edit) return;
    const current = text.getString();
    this.#first = current === '';
    edit.setString(current);
    edit.setVisible(true);
    edit.setFocus();
    text.setVisible(false);
  }

  /** `OnChildNotify( WIDC_BUTTON2 )`. Returns `false` for the refusal (the original's `return FALSE`). */
  send(): boolean {
    const typed = this.edit?.getString() ?? '';
    if (pledgeTooLong(typed)) {
      this.#deps.host.box(this.#deps.host.text(TID_GUILD_NOTICE_ERROR));
      return false;
    }
    if (!this.#first) {
      this.#ops.openConfirm({ pledge: typed, sent: () => undefined });
    } else {
      if (typed !== '') this.#deps.ports.actions.setPledge(typed);
      this.destroy();
    }
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_BUTTON2) {
      this.send();
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#ops.closeConfirm();
    super.onDestroy();
  }
}
