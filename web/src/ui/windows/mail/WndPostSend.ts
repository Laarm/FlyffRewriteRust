// `WndPostSend` (`CWndPostSend`, `src:_Interface/WndField.cpp:14590-15007`, ref/22 §17 I, Plan/42 §19.4 I, 45 §16; WP-UI-27): the "Send Mail" tab of the post window,
// `APP_POST_SEND` (287 x 320). Receiver, title, text (multi-line, `EBS_WANTRETURN | EBS_AUTOVSCROLL`), the item slot (`WIDC_STATIC3`: drops from the inventory
// only, a right click takes the item out), the penya (digits only) and the Send button. The receiver is a text field over the drop-down (`WIDC_COMBOBOX1`:
// the layout's combo cannot be typed in, so the field sits over its text area and the drop-down shows the friends when there are any). The English client
// moves the penya label 14 px left and widens the penya field (`GetLanguage() != LANG_KOR`).
//
// Send checks the form in the order of the original (`checkSendForm`: nothing is said for a missing receiver or title or penya that are not digits; the
// limits of penya, name, text and title open an upper message box), then asks "Would you like to send mail? Received mail will be deleted after 15 days." ;
// OK sends. KEEP-QUIRK: the original's check "an item or penya must be there" compared a byte with `NULL_ID` and never fired, so a mail may carry text only;
// after a send the fields keep their text (only the item is cleared).

import { APP_INVENTORY, APP_POST_SEND, WIDC_SEND } from '../../../gen/defs/resdata';
import { TID_MAIL_SEND_CONFIRM } from '../../../gen/defs/text';
import type { ComboBox } from '../../controls/ComboBox';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { Static } from '../../controls/Static';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { type Rect, rect } from '../../layout/geometry';
import {
  EBS_AUTOVSCROLL,
  EBS_MULTILINE,
  EBS_NUMBER,
  EBS_WANTRETURN,
  WBS_NOFRAME,
  WTYPE_EDITCTRL,
  Wnm,
} from '../../layout/styles';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import { MB, MBID } from '../../msgbox/types';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { LayoutPage } from './LayoutPage';
import { checkSendForm } from './form';
import { MailItemSlot } from './MailItemSlot';
import { WIDC_SEND_ITEM, WIDC_SEND_RECEIVER } from './names';
import { MAX_KEEP_MAX_DAY, type MailDraft, type MailSendForm, type SendPorts } from './ports';

export interface PostSendDeps {
  readonly ports: SendPorts;
  readonly msgbox: Pick<MessageBoxApi, 'open' | 'openUpper'>;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

/** The width of the drop button of the combo (`ButtQuickListDn.tga`). */
const DROP_BUTTON = 20;

export class WndPostSend extends LayoutPage {
  readonly appId = APP_POST_SEND;
  readonly #deps: PostSendDeps;
  #receiver: EditCtrl | null = null;
  #title: EditCtrl | null = null;
  #text: EditCtrl | null = null;
  #gold: EditCtrl | null = null;
  #combo: ComboBox | null = null;
  #slot: MailItemSlot | null = null;
  #draft: MailDraft | null = null;
  #friends: readonly string[] = [];
  #off: (() => void)[] = [];

  constructor(deps: PostSendDeps) {
    super();
    this.#deps = deps;
  }

  // ---- accessors for tests and the feature layer ------------------------------------------------------------------------------

  get receiverEdit(): EditCtrl | null {
    return this.#receiver;
  }
  get titleEdit(): EditCtrl | null {
    return this.#title;
  }
  get textEdit(): EditCtrl | null {
    return this.#text;
  }
  get goldEdit(): EditCtrl | null {
    return this.#gold;
  }
  get friendsCombo(): ComboBox | null {
    return this.#combo;
  }
  get itemSlot(): MailItemSlot | null {
    return this.#slot;
  }
  /** The form as typed. */
  form(): MailSendForm {
    return {
      receiver: this.#receiver?.getString() ?? '',
      title: this.#title?.getString() ?? '',
      text: this.#text?.getString() ?? '',
      gold: this.#gold?.getString() ?? '',
    };
  }

  /** `SetReceive`, `SetTitle`, `SetText`: the reply fills the form. */
  fill(f: { receiver?: string; title?: string; text?: string; gold?: string }): void {
    if (f.receiver !== undefined) this.#receiver?.setString(f.receiver);
    if (f.title !== undefined) this.#title?.setString(f.title);
    if (f.text !== undefined) this.#text?.setString(f.text);
    if (f.gold !== undefined) this.#gold?.setString(f.gold);
  }

  // ---- OnInitialUpdate ---------------------------------------------------------------------------------------------------------

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#title = this.getDlgItem<EditCtrl>('WIDC_EDIT2');
    this.#text = this.getDlgItem<EditCtrl>('WIDC_EDIT3');
    this.#gold = this.getDlgItem<EditCtrl>('WIDC_EDIT4');
    this.#combo = this.getDlgItem<ComboBox>('WIDC_COMBOBOX1');
    this.#text?.addWndStyle(EBS_MULTILINE | EBS_WANTRETURN | EBS_AUTOVSCROLL);
    this.#gold?.addWndStyle(EBS_NUMBER);

    // the English client: the penya label 14 px to the left, the field 14 px to the left and 10 px wider on the right
    const label = this.getDlgItem<Static>('WIDC_STATIC2');
    if (label) label.setWndRect(shift(label.rectWindow, -14, -14), true);
    if (this.#gold) this.#gold.setWndRect(shift(this.#gold.rectWindow, -14, 10), true);

    // the receiver: a text field over the combo's text area
    const slot = this.getWndCtrl('WIDC_COMBOBOX1');
    if (slot) {
      const r = slot.rect;
      this.#receiver = this.createControl<EditCtrl>(
        WTYPE_EDITCTRL,
        WIDC_SEND_RECEIVER,
        rect(r.l, r.t, r.r - DROP_BUTTON, r.b),
      );
    }
    this.#syncFriends();

    // the item slot over the picture of the layout
    const picture = this.getWndCtrl('WIDC_STATIC3');
    if (picture) {
      const s = new MailItemSlot({
        drop: (sc) => this.#drop(sc),
        rightClick: () => {
          this.#deps.ports.detach();
        },
        hover: (item, r) => {
          if (item && r) {
            this.#deps.tooltip.putItem(this, item, r, APP_POST_SEND, {});
          } else {
            this.#deps.tooltip.cancel();
          }
        },
      });
      // no frame: the whole picture of the slot takes the press and the drop
      this.#slot = this.addControl(s, WIDC_SEND_ITEM, picture.rect, WBS_NOFRAME);
    }
    this.#off.push(
      this.#deps.ports.draft.subscribe(() => {
        this.#applyDraft(this.#deps.ports.draft.get());
      }),
      this.#deps.ports.view.subscribe(() => {
        this.#syncFriends();
      }),
    );
    this.#applyDraft(this.#deps.ports.draft.get());
    this.#receiver?.setFocus();
  }

  #syncFriends(): void {
    const friends = this.#deps.ports.view.get().friends;
    if (friends === this.#friends) return;
    this.#friends = friends;
    const combo = this.#combo;
    if (!combo) return;
    combo.resetContent();
    for (const f of friends) combo.addString(f);
    // without friends there is nothing to drop down
    combo.setVisible(friends.length > 0);
  }

  #applyDraft(d: MailDraft): void {
    this.#draft = d;
    this.#slot?.set(d.item, d.count);
  }

  // ---- the item slot -------------------------------------------------------------------------------------------------------------

  /** `OnDropIcon`: an inventory item on the slot. */
  #drop(sc: Shortcut): boolean {
    const from = sc.from?.getFrameWnd() ?? null;
    if (sc.kind !== SC.ITEM || from?.id !== APP_INVENTORY) return false;
    if (sc.data === 0) {
      // the coin cannot be attached (the penya has its own field)
      this.env.wm.drag.forbid();
      return false;
    }
    const taken = this.#deps.ports.attach(sc);
    if (!taken) {
      this.env.wm.drag.forbid();
      return false;
    }
    // the original puts the name of the item into the title field when it differs
    const title = this.#title;
    if (
      title &&
      taken.name !== '' &&
      title.getString().toLowerCase() !== taken.name.toLowerCase()
    ) {
      title.setString(taken.name);
    }
    return true;
  }

  get draft(): MailDraft | null {
    return this.#draft;
  }

  // ---- Send --------------------------------------------------------------------------------------------------------------------------

  /** The Send button. Returns whether the confirmation box opened. */
  sendClicked(): boolean {
    const form = this.form();
    const check = checkSendForm(form);
    if (!check.ok) {
      if (check.kind === 'box') {
        const tpl = this.#deps.getText(check.tid);
        const text = check.argument === undefined ? tpl : tpl.replace(/%d/, String(check.argument));
        this.#deps.msgbox.openUpper(text);
      }
      return false;
    }
    // `CWndPostSendMessageBox`: OK / Cancel with the keep days filled in
    const ask = this.#deps.getText(TID_MAIL_SEND_CONFIRM).replace(/%d/, String(MAX_KEEP_MAX_DAY));
    this.#deps.msgbox.open(ask, MB.OKCANCEL, undefined, (id) => {
      if (id === MBID.OK) this.#deps.ports.send(this.form());
    });
    return true;
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.CLICKED && id === WIDC_SEND) {
      this.sendClicked();
      return true;
    }
    // a friend picked in the drop-down goes into the receiver field
    if (msg === Wnm.SELCHANGE && id === this.#combo?.id) {
      const name = this.#combo.getString();
      if (name !== '') this.#receiver?.setString(name);
      return true;
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    for (const off of this.#off.splice(0)) off();
    this.#deps.tooltip.cancel();
    super.onDestroy();
  }
}

function shift(r: Rect, dl: number, dr: number): Rect {
  return { l: r.l + dl, t: r.t, r: r.r + dr, b: r.b };
}
