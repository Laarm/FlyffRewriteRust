// The 3-D previews of the core windows (Plan/45 §7.2, §9.1, §20; WP-CFEAT-11): the paper doll of the inventory window
// (`WIDC_CUSTOM20`, preset `inventory`) and the head portrait of the status window (`WIDC_CUSTOM6`, preset `statusPortrait`).
// Both draw the OWN character, so the subject is the own `MoverVisualState` that the views hand to the shells; the renderer
// re-assembles it when an equip message edits it in place. The preset constants are 41's (`render/preview/presets.ts`); the
// runtime is the preview service of the title screens (`features/charselect/previews.ts`), built on the first request and
// shared by the paper doll and the portrait.

import type { ClientContext } from '../../../app/context';
import type { CorePreviewHandle, CorePreviewPort } from '../../../ui/windows/char/ports';
import { previewServiceOf } from '../../charselect/previews';

/** A `MoverVisualState` (the shells treat the subject as opaque): an object with an `equip` map and a `kind`. */
function isVisualState(subject: unknown): boolean {
  return (
    typeof subject === 'object' &&
    subject !== null &&
    'equip' in subject &&
    subject.equip instanceof Map &&
    'kind' in subject
  );
}

/**
 * `host.previews` of the core windows: `null` without WebGL2 (the canvases stay empty, 42 §8.12). A request without a subject
 * (the own mover is not in the world yet) opens nothing. The service is resolved at every request: the one of the title
 * screens is disposed when the world loads and a later request builds a new one.
 */
export function corePreviewPortOf(ctx: ClientContext): CorePreviewPort | null {
  if (!ctx.caps.webgl2) return null;
  return {
    open(request): CorePreviewHandle | null {
      if (!isVisualState(request.subject)) return null;
      return previewServiceOf(ctx).corePort.open(request);
    },
  };
}
