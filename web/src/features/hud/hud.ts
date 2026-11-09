// The HUD message state (Plan/45 §7.11): the centre client messages and the captions of one context, with the 60 Hz
// stepping and a version for the React view (`useSyncExternalStore`). The module registers `frame` with the tick systems.

import { Captions } from './captions';
import { CentreMessages } from './centre';

/** The two caption fonts (`m_pFontAPITitle` = viewport width / 20, `m_pFontAPICaption` = viewport width / 50, ENG). */
export const CAPTION_DIV_BIG = 20;
export const CAPTION_DIV_SMALL = 50;

/** Measures `text` in a font of `px` pixels with the caption outline (the DOM measurer in the browser). */
export type CaptionMeasure = (text: string, px: number) => { w: number; h: number };

export class HudState {
  readonly centre = new CentreMessages();
  readonly captions = new Captions();
  #version = 0;
  #listeners = new Set<() => void>();
  /** The virtual viewport width the caption font sizes derive from. */
  viewportWidth = 1024;
  /** The measurer of caption extents. */
  measure: CaptionMeasure = (text, px) => ({
    w: Math.round(text.length * px * 0.6),
    h: Math.round(px * 1.25),
  });

  readonly subscribe = (fn: () => void): (() => void) => {
    this.#listeners.add(fn);
    return () => {
      this.#listeners.delete(fn);
    };
  };

  /** A number that changes whenever the view must re-render. */
  readonly getVersion = (): number => this.#version;

  #bump(): void {
    this.#version += 1;
    for (const fn of [...this.#listeners]) fn();
  }

  /** The caption font size in px of the big and the small caption. */
  captionPx(small: boolean): number {
    return Math.max(
      1,
      Math.trunc(this.viewportWidth / (small ? CAPTION_DIV_SMALL : CAPTION_DIV_BIG)),
    );
  }

  clientMsg(text: string, color: number, nowMs: number): void {
    this.centre.add(text, color, nowMs);
    this.#bump();
  }

  /** Drops the centre lines with this text (the cause is gone: the death line after the revival); `true` when one went. */
  removeClientMsg(text: string): boolean {
    const removed = this.centre.remove(text) > 0;
    if (removed) this.#bump();
    return removed;
  }

  /** `AddCaption`; the extent is measured in the caption font of the current viewport. */
  caption(text: string, small: boolean, nowMs: number): void {
    const size = this.measure(text, this.captionPx(small));
    this.captions.add(text, small, size, nowMs);
    this.#bump();
  }

  /** `steps` 60 Hz frames of the captions and of the centre messages. */
  frame(steps: number, nowMs: number): void {
    let changed = false;
    for (let i = 0; i < steps; i += 1) {
      if (this.centre.frame(nowMs)) changed = true;
      if (this.captions.list.length > 0) {
        this.captions.frame(nowMs);
        changed = true;
      }
    }
    if (changed) this.#bump();
  }

  // ---- rainbow race and quiz (ui-rainbow)
  /** `g_Caption1.RemoveAll()` without the centre messages: the quiz countdown replaces the digit it shows every second. */
  clearCaptions(): void {
    this.captions.clear();
    this.#bump();
  }

  /** The world was left: nothing floats over the next one (`g_ClientMsg.ClearAllMessage`, `g_Caption1.RemoveAll`). */
  clear(): void {
    this.centre.clear();
    this.captions.clear();
    this.#bump();
  }
}
