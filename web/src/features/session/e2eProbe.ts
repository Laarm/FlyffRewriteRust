// Test-only members of `window.__flyffDebug.features` for the Playwright specs of the login-state screens (present only when
// the probe has test hooks, `?e2e=1` or a dev build): the specs click controls and read message boxes instead of guessing
// pixels (Plan/41 §21 and 45 §2.6: "Playwright reads the probe, not pixels").
//
//   controlPoint(app, id, key?)   the CSS pixel centre of a control of an open window, `null` when it is not there
//   boxText()                     the plain text of the standard message box on the screen, `null` when none
//   countdownText()               the text of the logout countdown window ("Quit after N sec."), `null` when none
//   editText(app, id, key?)       the text of an edit control (`""` for a password field is NOT hidden: e2e inputs only)

import { APP_MESSAGE_TEXT, APP_MESSAGEBOX } from '../../gen/defs/resdata';
import { LogoutCountdownWnd } from '../../ui/session';
import type { MessageBoxWnd } from '../../ui/msgbox/MessageBoxWnd';
import { EditCtrl } from '../../ui/controls/EditCtrl';
import type { FeatureContext } from '../common/module';
import { addProbeMember } from '../common/probe';

export function installLobbyE2eProbe(fc: FeatureContext): void {
  const ctx = fc.app;
  if (!ctx.probe.test) return;
  const control = (app: number, id: number, key: string) => {
    const w = fc.ui?.wm.get(app, key) ?? null;
    return w?.controls.find((c) => c.id === id) ?? null;
  };
  addProbeMember(ctx, 'controlPoint', (app: number, id: number, key?: string) => {
    const c = control(app, id, key ?? '');
    if (!c) return null;
    const r = c.getScreenRect();
    const scale = fc.ui?.scaler.get().scale ?? 1;
    const root = document.querySelector('[data-testid="ui-root"]')?.getBoundingClientRect();
    const left = root?.left ?? 0;
    const top = root?.top ?? 0;
    return {
      x: Math.round(left + ((r.l + r.r) / 2) * scale),
      y: Math.round(top + ((r.t + r.b) / 2) * scale),
    };
  });
  addProbeMember(ctx, 'boxText', () => {
    const box = fc.ui?.wm.get(APP_MESSAGEBOX, 'main') as MessageBoxWnd | null | undefined;
    return box?.textCtrl?.getString() ?? null;
  });
  addProbeMember(ctx, 'countdownText', () => {
    const w = fc.ui?.wm.get(APP_MESSAGE_TEXT);
    return w instanceof LogoutCountdownWnd ? (w.textCtrl?.getString() ?? null) : null;
  });
  addProbeMember(ctx, 'editText', (app: number, id: number, key?: string) => {
    const c = control(app, id, key ?? '');
    return c instanceof EditCtrl ? c.getString() : null;
  });
}
