// The turn of the weapon in the jewel setting window (`CWndSmeltJewel::Process`, `m_nStatus == 1`; WP-UI-32): the counters of the original run to
// the end. Pure, so the service, the tests and the shell share it without the shell being imported.

/** The frame rate the original's per-frame counters assume. */
export const SPIN_FPS = 60;

export interface SpinTimeline {
  /** Frames from Start to the send (`m_nStatus = 1` until `m_nDelay < 0`). */
  readonly frames: number;
  /** The total rotation in degrees (`Σ m_fAddRot`). */
  readonly degrees: number;
  /** The frame at which the pale quad starts to fade in (`m_nDelay < 10`). */
  readonly flashFrame: number;
  /** `frames` in ms at {@link SPIN_FPS}. */
  readonly ms: number;
}

/**
 * The counters of `CWndSmeltJewel::Process` for `m_nStatus == 1`, run to the end: `m_nCount` counts frames up to `m_nDelay` (25 first), each
 * time it passes the delay the rotation speed gains 4 degrees per frame and the delay shrinks by one; below 10 the quad fades in; below 0 the
 * request is sent.
 */
export function spinTimeline(): SpinTimeline {
  let count = 0;
  let delay = 25;
  let addRot = 2;
  let frames = 0;
  let degrees = 0;
  let flashFrame = -1;
  for (;;) {
    frames += 1;
    degrees += addRot;
    if (count > delay) {
      addRot += 4;
      if (delay < 10 && flashFrame < 0) flashFrame = frames;
      delay -= 1;
      if (delay < 0) break;
      count = 0;
    }
    count += 1;
  }
  return { frames, degrees, flashFrame, ms: Math.round((frames * 1000) / SPIN_FPS) };
}
