// `BankLogList` (`CWndAddItemLog`, `CWndRemoveItemLog`, `CWndReceivePenyaLog`, `CWndInvestPenyaLog`, `src:_Interface/WndGuildBank.cpp:455-760`; WP-UI-47): one tab page of the
// guild bank log: the lines of a list, 20 px apart from (4, 3), black text, with a docked scroll bar (one row per wheel notch). The four pages of the original were four copies of
// the same class; here they are four instances. Display only: the lines come from the view (the feature keeps them whether or not the window is open).

import { viewOf } from '../../controls/bind';
import { RowList } from '../../lists/RowList';
import { BankLogListView, LOG_PITCH, LOG_TOP, LOG_X } from './BankLogListView';

export { LOG_PITCH, LOG_TOP, LOG_X };

export class BankLogList extends RowList<string> {
  constructor() {
    super(
      { pitch: LOG_PITCH, top: LOG_TOP, inset: 0, bar: true, frame: false },
      {
        rows: [],
        hover: -1,
        selected: -1,
        scroll: 0,
        page: 1,
        clientOffset: { x: 0, y: 0 },
      },
    );
    this.view = viewOf<BankLogList>(BankLogListView);
  }

  /** The lines of the list (newest first); the scroll position goes back to the top (`UpdateScroll`: `SetScrollPos( 0 )`). */
  setLines(lines: readonly string[]): void {
    if (lines === this.rows) return;
    this.setRowsState(lines, { scroll: 0 });
    this.scrollTo(0);
  }

  get lines(): readonly string[] {
    return this.rows;
  }
}
