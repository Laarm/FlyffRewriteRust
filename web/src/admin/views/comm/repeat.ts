// Plain descriptions of a notice's schedule and audience for the list and the preview (Plan/25 section 8.1). Pure.
import type { NoticeRepeatDto, NoticeTargetDto } from '../../gen/dto';
import { T } from './strings';

const N = T.notices;

/** The bytes of a text as the game counts them (UTF-8). */
export function utf8Length(text: string): number {
  return new TextEncoder().encode(text).length;
}

/** `once`, `every 10 minutes`, `every day at 18:30`. */
export function describeRepeat(r: NoticeRepeatDto): string {
  switch (r.kind) {
    case 'once':
      return N.onceText;
    case 'every':
      return N.everyN(r.minutes);
    case 'daily':
      return N.dailyText(r.at);
  }
}

/** `everybody`, the name of the world, `3 characters`. */
export function describeTarget(
  t: NoticeTargetDto,
  worldName: (id: number) => string = (id) => `World ${String(id)}`,
): string {
  switch (t.kind) {
    case 'all':
      return N.targetAll;
    case 'world':
      return worldName(t.world);
    case 'characters':
      return N.characters(t.characters.length);
  }
}

/** The tone of a state badge. */
export function stateTone(state: string): 'ok' | 'accent' | 'warn' | 'neutral' | 'danger' {
  switch (state) {
    case 'scheduled':
      return 'accent';
    case 'done':
      return 'ok';
    case 'paused':
      return 'warn';
    case 'missed':
      return 'danger';
    default:
      return 'neutral';
  }
}
