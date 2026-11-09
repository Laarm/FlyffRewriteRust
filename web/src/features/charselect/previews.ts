// The per-context preview service (see `previewHost.ts`): created on the first request of a character window, disposed when
// the world loads (the world renderer takes over, 40 §9.8) or on request. `null` port when WebGL2 is missing.

import type { ClientContext } from '../../app/context';
import type { PreviewPort } from '../../ui/windows/login/ports';
import { createPreviewService, type PreviewService, type PreviewServiceDeps } from './previewHost';

const services = new WeakMap<ClientContext, PreviewService>();
let testDeps: PreviewServiceDeps | undefined;

/** Test seam: the runtime factory used for services created afterwards. */
export function setPreviewServiceDeps(deps: PreviewServiceDeps | undefined): void {
  testDeps = deps;
}

/** The service of a context (created on first use). */
export function previewServiceOf(ctx: ClientContext): PreviewService {
  let s = services.get(ctx);
  if (!s) {
    s = createPreviewService(ctx, testDeps);
    services.set(ctx, s);
  }
  return s;
}

/** The port of the shells; `null` when this browser has no WebGL2 (the canvases then stay empty, 42 §8.12). */
export function previewPortOf(ctx: ClientContext): PreviewPort | null {
  if (!ctx.caps.webgl2) return null;
  return previewServiceOf(ctx).port;
}

/** Disposes the service of a context (the renderer and its GL context go away); a later request builds a new one. */
export function disposePreviews(ctx: ClientContext): void {
  const s = services.get(ctx);
  if (!s) return;
  services.delete(ctx);
  s.dispose();
}
