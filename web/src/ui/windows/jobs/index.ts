// `JobWindowsService` (`ui.jobs`, Plan/42 §19.4 D / S; WP-UI-38): the fast job change window and the job transfer windows of the
// class-change scroll.
//
//   ui.jobs.openFastJobChange(request)   APP_FASTJOBCHANGE "Select Class": the classes on offer, one at a time
//   ui.jobs.openChangeClass(request)     APP_CHANGECLASS_1 for a first job, APP_CHANGECLASS_2 for a second job; none for any other job
//
// Each `open*` replaces a window of the same kind that is still open (the originals `SAFE_DELETE` their member first).

import { APP_CHANGECLASS_1, APP_CHANGECLASS_2, APP_FASTJOBCHANGE } from '../../../gen/defs/resdata';
import type { WindowManager } from '../../wm/WindowManager';
import { type LazyWnd, openLazy, type WndClass } from '../../wnd/lazy';
import { changeClassWindowFor } from './classJobs';
import { JOBS_LAZY } from './lazy';
import type { ChangeClassRequest, FastJobRequest } from './ports';
import type { WndChangeClass1, WndChangeClass2 } from './WndChangeClass';
import type { WndFastJobChange } from './WndFastJobChange';

export * from './ports';
export { ERROR_TITLE, ERROR_TOOLTIP } from './fastJobTexts';
export type { WndFastJobChange } from './WndFastJobChange';
export type { WndChangeClass1, WndChangeClass2 } from './WndChangeClass';
export { changeClassWindowFor } from './classJobs';
export { jobPicture, ERROR_PICTURE } from './jobPictures';

export interface JobWindowsDeps {
  readonly wm: WindowManager;
}

export class JobWindowsService {
  readonly #wm: WindowManager;

  constructor(deps: JobWindowsDeps) {
    this.#wm = deps.wm;
  }

  /** Opens the window of a lazy class (`null` while its chunk is still loading: the window then opens by itself). */
  #replace<C extends WndClass>(
    app: number,
    lazy: LazyWnd<C>,
    build: (ctor: C) => InstanceType<C>,
  ): InstanceType<C> | null {
    return openLazy(this.#wm, app, lazy, build, (a) => {
      const old = this.#wm.get(a);
      if (!old) return;
      old.destroy();
      this.#wm.flushDestroyed();
    });
  }

  openFastJobChange(request: FastJobRequest): WndFastJobChange | null {
    return this.#replace(APP_FASTJOBCHANGE, JOBS_LAZY.WndFastJobChange, (C) => new C(request));
  }

  /** The open fast job change window, `null` when none. */
  fastJobChange(): WndFastJobChange | null {
    const w = this.#wm.get(APP_FASTJOBCHANGE);
    const ctor = JOBS_LAZY.WndFastJobChange.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  /**
   * The scroll's window for the job of the request: `APP_CHANGECLASS_1` for the four first jobs, `APP_CHANGECLASS_2` for the eight
   * second jobs, `null` (nothing opens) for a master, a hero or any job without a radio.
   */
  openChangeClass(request: ChangeClassRequest): WndChangeClass1 | WndChangeClass2 | null {
    const which = changeClassWindowFor(request.job);
    if (which === 1) {
      return this.#replace(APP_CHANGECLASS_1, JOBS_LAZY.WndChangeClass1, (C) => new C(request));
    }
    if (which === 2) {
      return this.#replace(APP_CHANGECLASS_2, JOBS_LAZY.WndChangeClass2, (C) => new C(request));
    }
    return null;
  }

  changeClass1(): WndChangeClass1 | null {
    const w = this.#wm.get(APP_CHANGECLASS_1);
    const ctor = JOBS_LAZY.WndChangeClass1.ctor;
    return ctor && w instanceof ctor ? w : null;
  }

  changeClass2(): WndChangeClass2 | null {
    const w = this.#wm.get(APP_CHANGECLASS_2);
    const ctor = JOBS_LAZY.WndChangeClass2.ctor;
    return ctor && w instanceof ctor ? w : null;
  }
}
