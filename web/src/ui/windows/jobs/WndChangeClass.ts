// `WndChangeClass1` / `WndChangeClass2` (`CWndChangeClass1`, `CWndChangeClass2`, `src:_Interface/WndField.cpp:13440-13760`, ref/22
// §17 D, Plan/42 §19.4 D, 45 §21.1; WP-UI-38): `APP_CHANGECLASS_1` (first jobs: Mercenary, Acrobat, Assist, Magician) and
// `APP_CHANGECLASS_2` (second jobs: Knight, Blade, Jester, Ranger, Ringmaster, Billposter, Psykeeper, Elementor), "Job Transfer"
// (224 x 192), opened by the class-change scroll. The own job is pre-checked and its radio disabled; the window is centred. OK on
// the own job prints `TID_GAME_EQUALJOB` and stays open, OK on another job closes it (the legacy `SendChangeJob` reaches a server
// handler that does not exist, so the port sends nothing); Cancel and the close button close.

import { APP_CHANGECLASS_1, APP_CHANGECLASS_2 } from '../../../gen/defs/resdata';
import type { Button } from '../../controls/Button';
import type { Shortcut } from '../../dnd/shortcut';
import type { WidcKey } from '../../layout/registry';
import { WTBID_CLOSE, Wnm } from '../../layout/styles';
import type { WndBase } from '../../wnd/WndBase';
import { WndNeuz } from '../../wnd/WndNeuz';
import { CLASS1, CLASS2, type Radio, changeClassWindowFor } from './classJobs';
import type { ChangeClassRequest } from './ports';

export { changeClassWindowFor };

abstract class WndChangeClass extends WndNeuz {
  protected abstract readonly radios: readonly Radio[];
  protected abstract readonly okName: WidcKey;
  protected abstract readonly cancelName: WidcKey;
  readonly #request: ChangeClassRequest;
  /** `nJob`: the radio that is checked. */
  #job: number;

  constructor(request: ChangeClassRequest) {
    super();
    this.#request = request;
    this.#job = request.job;
    this.putRegInfo = false;
  }

  /** The job that is checked. */
  get selectedJob(): number {
    return this.#job;
  }

  /** The radio button of a job (`null` when the window has none). */
  radio(job: number): Button | null {
    const r = this.radios.find((x) => x.job === job);
    return r ? this.getDlgItem<Button>(r.name) : null;
  }
  get okButton(): Button | null {
    return this.getDlgItem<Button>(this.okName);
  }
  get cancelButton(): Button | null {
    return this.getDlgItem<Button>(this.cancelName);
  }

  override onInitialUpdate(): void {
    super.onInitialUpdate();
    this.#setJob();
    this.radio(this.#request.job)?.enableWindow(false);
    this.moveParentCenter();
  }

  /** `SetJob`: exactly one radio is checked, the one of `nJob`. */
  #setJob(): void {
    for (const r of this.radios) {
      this.getDlgItem<Button>(r.name)?.setCheck(r.job === this.#job);
    }
  }

  /** OK (`WIDC_BUTTON_OK` / Enter). */
  ok(): void {
    if (this.#request.job === this.#job) {
      this.#request.equalJob();
      return;
    }
    this.#request.select(this.#job);
    this.destroy();
  }

  override onChildNotify(msg: number, id: number, src: WndBase | Shortcut | null): boolean {
    if (msg === Wnm.EN_RETURN || (msg === Wnm.CLICKED && id === this.okButton?.id)) {
      this.ok();
      return true;
    }
    if (msg === Wnm.CLICKED) {
      if (id === this.cancelButton?.id || id === WTBID_CLOSE) {
        this.destroy();
        return true;
      }
      const r = this.radios.find((x) => this.getDlgItem(x.name)?.id === id);
      if (r) {
        this.#job = r.job;
        this.#setJob();
        return true;
      }
    }
    return super.onChildNotify(msg, id, src);
  }

  override onDestroy(): void {
    this.#request.closed?.();
    super.onDestroy();
  }
}

/** `CWndChangeClass1`. */
export class WndChangeClass1 extends WndChangeClass {
  readonly appId = APP_CHANGECLASS_1;
  protected readonly radios = CLASS1;
  protected readonly okName = 'WIDC_BUTTON_OK' as const;
  protected readonly cancelName = 'WIDC_BUTTON_CANCEL' as const;
}

/** `CWndChangeClass2`. */
export class WndChangeClass2 extends WndChangeClass {
  readonly appId = APP_CHANGECLASS_2;
  protected readonly radios = CLASS2;
  protected readonly okName = 'WIDC_BUTTON_OK2' as const;
  protected readonly cancelName = 'WIDC_BUTTON_CANCEL2' as const;
}
