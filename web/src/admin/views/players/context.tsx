// What the tabs of one character's drawer share: the live state of the character (profile, details, containers) and who the
// console is looking at. One `useCharacterLive` per open drawer; the tabs read it from here.
import { createContext, useContext, type ReactNode } from 'react';
import type { CharacterLive } from './live';

export interface CharacterCtx {
  readonly id: number;
  readonly name: string;
  readonly live: CharacterLive;
}

const Ctx = createContext<CharacterCtx | null>(null);

export function CharacterProvider({
  value,
  children,
}: {
  readonly value: CharacterCtx;
  readonly children: ReactNode;
}) {
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

/** The character of the enclosing drawer. */
export function useCharacter(): CharacterCtx {
  const v = useContext(Ctx);
  if (!v) throw new Error('useCharacter outside a character drawer');
  return v;
}
