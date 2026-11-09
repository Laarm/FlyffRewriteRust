// Typed view binding: a control class sets `this.view = viewOf(MyView)` where `MyView` takes `{ control: MyControl }`
// (the frame view hands every control view `{ control: Control }`; the class knows which control it renders).

import type { ComponentType } from 'react';
import type { ControlViewProps } from '../wnd/Control';

// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- the parameter names the control class
export function viewOf<C>(view: ComponentType<{ control: C }>): ComponentType<ControlViewProps> {
  return view as unknown as ComponentType<ControlViewProps>;
}
