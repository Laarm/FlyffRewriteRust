// Badge: a small status label. Tones carry meaning with text as well (never colour alone).
import type { ReactNode } from 'react';

export type Tone = 'neutral' | 'accent' | 'ok' | 'warn' | 'danger';

export function Badge({
  tone = 'neutral',
  children,
  title,
}: {
  readonly tone?: Tone;
  readonly children: ReactNode;
  readonly title?: string;
}) {
  return (
    <span className={`adm-badge adm-badge--${tone}`} {...(title === undefined ? {} : { title })}>
      {children}
    </span>
  );
}
