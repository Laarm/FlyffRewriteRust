// `WndPostRead` (`CWndPostRead`, `src:_Interface/WndField.cpp:15011-15447`, ref/22 §17 I, Plan/42 §19.4 I, 45 §16; WP-UI-27): `APP_POST_READ` (288 x 336), modal.
// The sender and the title in two disabled fields (cut at 16 and 13 bytes with "..."), the text in the scrollable text control, the item slot and the penya
// (with thousands separators). The item is taken by dragging it onto the inventory or with a right click on it (a custody fee asks first), the penya with a
// right click on the coin; Delete asks for the confirmation, Reply gives the send tab the sender and "Re: " + the title, Close closes. The mail is followed
// by id: when it disappears from the box (deleted, expired) the reader closes. FIX: the text is shown as typed (the original parsed the markup codes of the
// sender's text), and a reply keeps the whole title (the original took the cut one with its dots).

import { APP_POST_READ, WIDC_BUTTON1, WIDC_BUTTON2, WIDC_BUTTON3 } from '../../../gen/defs/resdata';
import { TID_MAIL_UNKNOW } from '../../../gen/defs/text';
import type { Button } from '../../controls/Button';
import type { EditCtrl } from '../../controls/EditCtrl';
import type { TextCtrl } from '../../controls/TextCtrl';
import { SC, type Shortcut } from '../../dnd/shortcut';
import { groupThousands } from '../../lists/geometry';
import { WBS_MODAL, WBS_NOFRAME, Wnm } from '../../layout/styles';
import type { MessageBoxApi } from '../../msgbox/MessageBoxService';
import type { TooltipService } from '../../tooltip/TooltipService';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { BANK_GOLD_ICON } from '../bank/BankGoldIcon';
import { cutText } from './form';
import { MailItemSlot } from './MailItemSlot';
import { WIDC_READ_COIN, WIDC_READ_ITEM } from './names';
import type { MailRow, ReadRequest } from './ports';

export interface PostReadDeps {
  readonly request: ReadRequest;
  readonly msgbox: Pick<MessageBoxApi, 'openUpper'>;
  readonly tooltip: TooltipService;
  readonly getText: (tid: number) => string;
}

export class WndPostRead extends WndNeuz {
  readonly appId = APP_POST_READ;
  readonly #deps: PostReadDeps;
  #sender: EditCtrl | null = null;
  #title: EditCtrl | null = null;
  #gold: EditCtrl | null = null;
  #body: TextCtrl | null = null;
  #item: MailItemSlot | null = null;
  #coin: MailItemSlot | null = null;
  #shown: MailRow | null = null;
  #closed = false;

  constructor(deps: PostReadDeps) {
    super();
    this.#deps = deps;
    this.putRegInfo = false;
  }

  override initialize(parent: WndBase, extraStyle = 0): boolean {
    return super.initialize(parent, WBS_MODAL | extraStyle);
  }

  // ---- accessors for tests and the feature layer -----------------------------------------------------------------------------

  get senderEdit(): EditCtrl | null {
    return this.#sender;
  }
  get titleEdit(): EditCtrl | null {
    return this.#title;
  }
  get goldEdit(): EditCtrl | null {
    return this.#gold;
  }
  get body(): TextCtrl | null {
    return this.#body;
  }
  get itemSlot(): MailItemSlot | null {
    return this.#item;
  }
  get coinSlot(): MailItemSlot | null {
    return this.#coin;
  }
  /** The mail this reader shows. */
  get mailId(): number | null {
    return this.#shown?.id ?? null;
  }
  get deleteButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON1);
  }
  get replyButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON2);
  }
  get closeButton(): Button | null {
    return this.getDlgItem<Button>(WIDC_BUTTON3);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#sender = this.getDlgItem<EditCtrl>('WIDC_EDIT1');
    this.#title = this.getDlgItem<EditCtrl>('WIDC_EDIT2');
    this.#gold = this.getDlgItem<EditCtrl>('WIDC_EDIT3');
    this.#body = this.getDlgItem<TextCtrl>('WIDC_TEXT1');
    // the penya field is 32 px wider on the right
    if (this.#gold) {
      const r = this.#gold.rectWindow;
      this.#gold.setWndRect({ l: r.l, t: r.t, r: r.r + 32, b: r.b }, true);
    }
    for (const e of [this.#sender, this.#title, this.#gold]) e?.enableWindow(false);
    this.closeButton?.setFocus();

    const picture = this.getWndCtrl('WIDC_STATIC3');
    if (picture) {
      this.#item = this.addControl(
        new MailItemSlot({
          frameOnHover: true,
          shortcutOf: () => (this.#shown?.item ? MailItemSlot.payload(this.#shown.item) : null),
          rightClick: () => {
            if (this.#shown) this.#deps.request.actions.takeItem(this.#shown.id);
          },
          hover: (item, r) => {
            if (item && r) this.#deps.tooltip.putItem(this, item, r, APP_POST_READ, {});
            else this.#deps.tooltip.cancel();
          },
        }),
        WIDC_READ_ITEM,
        picture.rect,
        WBS_NOFRAME,
      );
    }
    const coin = this.getWndCtrl('WIDC_STATIC4');
    if (coin) {
      this.#coin = this.addControl(
        new MailItemSlot({
          frameOnHover: true,
          shortcutOf: () =>
            this.#shown && this.#shown.gold > 0
              ? {
                  kind: SC.ITEM,
                  id: 0,
                  type: 0,
                  index: 0xffff_ffff,
                  userId: 0,
                  data: 0,
                  icon: BANK_GOLD_ICON,
                }
              : null,
          rightClick: () => {
            if (this.#shown) this.#deps.request.actions.takeGold(this.#shown.id);
          },
        }),
        WIDC_READ_COIN,
        coin.rect,
        WBS_NOFRAME,
      );
    }
    this.moveParentCenter();
    this.#apply();
  }

  // ---- the view ----------------------------------------------------------------------------------------------------------------

  override process(now: number): void {
    this.#apply();
    super.process(now);
  }

  #apply(): void {
    if (this.#closed) return;
    const mail = this.#deps.request.mail();
    if (!mail) {
      // the mail is gone (deleted, expired): the reader goes with it
      if (this.#shown) {
        this.#closed = true;
        this.destroy();
      }
      return;
    }
    if (mail === this.#shown) return;
    const first = this.#shown === null;
    this.#shown = mail;
    // `SetValue`: the sender (16 bytes), the title (13), the text, the penya with separators
    this.#sender?.setString(mail.senderKnown ? cutText(mail.sender, 16, 'over') : 'Unknown');
    this.#title?.setString(cutText(mail.title, 13, 'atLeast'));
    if (first || this.#body?.getString() !== mail.text) this.#body?.setLiteral(mail.text);
    this.#gold?.setString(groupThousands(mail.gold));
    this.#item?.set(mail.item, mail.item?.count ?? null);
    this.#coin?.setActive(mail.gold > 0);
  }

  /** `CWndPostRead::OnDropIcon` returns `TRUE` for everything: a drop on the reader is taken and does nothing. */
  override onDropIcon(): boolean {
    return true;
  }

  // ---- the buttons -------------------------------------------------------------------------------------------------------------

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    const mail = this.#shown;
    if (msg === Wnm.CLICKED && mail) {
      if (id === WIDC_BUTTON1) {
        // Delete: the scroll of a sealed character cannot go (`II_SYS_SYS_SCR_SEALCHARACTER`)
        if (mail.deletable) this.#deps.request.actions.confirmDelete(mail.id);
        return true;
      }
      if (id === WIDC_BUTTON2) {
        if (!mail.senderKnown) {
          this.#deps.msgbox.openUpper(this.#deps.getText(TID_MAIL_UNKNOW));
          return true;
        }
        this.#deps.request.actions.reply(mail.id);
        return true;
      }
      if (id === WIDC_BUTTON3) {
        this.destroy();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#deps.tooltip.cancel();
    this.#deps.request.actions.closed();
    super.onDestroy();
  }
}
