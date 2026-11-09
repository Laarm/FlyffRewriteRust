// A window with no behaviour of its own: the frame, title bar and controls of one layout. Used by the `/dev/ui`
// gallery (any APP id can be shown before its shell exists) and by tests.

import { WndNeuz } from './WndNeuz';

export class LayoutWindow extends WndNeuz {
  readonly appId: number;

  constructor(appId: number) {
    super();
    this.appId = appId;
  }
}
