// The 3-D previews of the pet windows (Plan/41 §21; WP-UI-33, WP-UI-34): the model of the summoned pet in the status window and of the eat pet in the vis-pet window.
// The runtime is the preview service of the title screens (`features/charselect/previews.ts`), built on the first request; a subject that is no `MoverVisualState`
// (the pet is not in view yet) opens nothing.

import type { ClientContext } from '../../app/context';
import type { PreviewPort } from '../../ui/windows/login/ports';
import { previewServiceOf } from '../charselect/previews';

function isVisualState(subject: unknown): boolean {
  return (
    typeof subject === 'object' &&
    subject !== null &&
    'equip' in subject &&
    subject.equip instanceof Map &&
    'kind' in subject
  );
}

/** `host.previews` of the pet windows: `null` without WebGL2 (the canvases stay empty, 42 §8.12). */
export function petPreviewPortOf(ctx: ClientContext): PreviewPort | null {
  if (!ctx.caps.webgl2) return null;
  return {
    open(request) {
      if (!isVisualState(request.subject)) return null;
      return previewServiceOf(ctx).port.open(request);
    },
  };
}
