// The element of an edit control (Plan/42 §8.7): a transparent `<input>` / `<textarea>` over the client rect, `FONT.text`,
// black text and caret. The control owns the semantics (`EditCtrl.domInput` / `domKeyDown`); this view only wires the DOM.

import { useCallback, useSyncExternalStore, type ChangeEvent, type KeyboardEvent } from 'react';
import { rectHeight, rectWidth } from '../layout/geometry';
import { EBS_MULTILINE, EBS_NUMBER, EBS_PASSWORD, EBS_READONLY } from '../layout/styles';
import { FONT, fontStyle, lineHeight } from '../text/fonts';
import type { EditCtrl } from './EditCtrl';

export function EditCtrlView({ control }: { control: EditCtrl }) {
  const s = useSyncExternalStore(control.state.subscribe, control.state.get);
  const w = control.rectWindow;
  const c = control.rectClient;
  const multi = (s.style & EBS_MULTILINE) !== 0;
  const cell = lineHeight(FONT.text);
  const attach = useCallback(
    (el: HTMLInputElement | HTMLTextAreaElement | null) => {
      control.attach(el);
    },
    [control],
  );
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>): void => {
    if (control.domKeyDown(e.code, e.shiftKey)) e.preventDefault();
  };
  const onChange = (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>): void => {
    control.domInput(e.target.value);
  };
  const common = {
    ref: attach,
    value: s.value,
    disabled: !s.enabled,
    readOnly: (s.style & EBS_READONLY) !== 0,
    ...(s.maxLength > 0 ? { maxLength: s.maxLength } : {}),
    spellCheck: false,
    autoComplete: 'off',
    onChange,
    onKeyDown,
    onFocus: () => {
      control.domFocus();
    },
    style: {
      left: c.l - w.l,
      top: c.t - w.t,
      width: Math.max(0, rectWidth(c)),
      height: multi ? Math.max(0, rectHeight(c)) : Math.min(cell, Math.max(0, rectHeight(c))),
      ...fontStyle(FONT.text),
    },
  } as const;
  return (
    <div className="ed" data-testid="edit" style={{ width: rectWidth(w), height: rectHeight(w) }}>
      {multi ? (
        <textarea className="ed-field" {...common} />
      ) : (
        <input
          className="ed-field"
          type={(s.style & EBS_PASSWORD) !== 0 ? 'password' : 'text'}
          inputMode={(s.style & EBS_NUMBER) !== 0 ? 'numeric' : 'text'}
          {...common}
        />
      )}
    </div>
  );
}
