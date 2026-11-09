// The chat styles and the default channel masks (Plan/42 §13.2, 45 §8.1; `WndManager.h:143-157`, `HwOption.cpp:226-231`).

/** `CHATSTY_*`: the style of a chat line. A channel style is one bit; HELP, SYSTEM and GAME are bit pairs. */
export const CHATSTY = {
  GENERAL: 0x1,
  WHISPER: 0x2,
  SHOUT: 0x4,
  PARTY: 0x8,
  GUILD: 0x10,
  HELP: 0x60,
  HELP_CLIENT: 0x20,
  HELP_CHAT: 0x40,
  SYSTEM: 0x180,
  SYSTEM_CLIENT: 0x80,
  SYSTEM_CHAT: 0x100,
  GAME: 0x600,
  GAME_CLIENT: 0x200,
  GAME_CHAT: 0x400,
} as const;

/** The five channels of the chat tabs in tab order: general, whisper, shout, party, guild. */
export const CHANNELS = [
  CHATSTY.GENERAL,
  CHATSTY.WHISPER,
  CHATSTY.SHOUT,
  CHATSTY.PARTY,
  CHATSTY.GUILD,
] as const;

/** `m_dwChatFilter[0..4]` defaults: tab 0 shows everything, the others general plus their own channel. */
export const DEFAULT_CHAT_FILTERS: readonly number[] = [
  CHATSTY.GENERAL | CHATSTY.WHISPER | CHATSTY.SHOUT | CHATSTY.PARTY | CHATSTY.GUILD,
  CHATSTY.GENERAL | CHATSTY.WHISPER,
  CHATSTY.GENERAL | CHATSTY.SHOUT,
  CHATSTY.GENERAL | CHATSTY.PARTY,
  CHATSTY.GENERAL | CHATSTY.GUILD,
];

/** `m_dwChatFilterEtc` default: help and system lines in the chat box, game messages as centre client messages. */
export const DEFAULT_CHAT_FILTER_ETC =
  CHATSTY.HELP_CHAT | CHATSTY.SYSTEM_CHAT | CHATSTY.GAME_CLIENT;

/** The bits the loader strips from the channel filters (`&= ~(HELP | SYSTEM | GAME)`, HwOption.cpp:917-942). */
export const ETC_BITS = CHATSTY.HELP | CHATSTY.SYSTEM | CHATSTY.GAME;

/** The effective masks: five stored filters mean "set by the user" (both parts), anything else the legacy defaults. */
export function effectiveChatFilters(
  filters: readonly number[],
  etc: number,
): { filters: readonly number[]; etc: number } {
  if (filters.length !== 5) return { filters: DEFAULT_CHAT_FILTERS, etc: DEFAULT_CHAT_FILTER_ETC };
  return { filters: filters.map((f) => (f & ~ETC_BITS) >>> 0), etc: etc >>> 0 };
}

/** The edit prefill of a tab (`WndCommunication.cpp:38-64`, 691-716). */
export function tabPrefill(tab: number, lastWhisperer: string | null): string {
  switch (tab) {
    case 1:
      return lastWhisperer && lastWhisperer !== '' ? `/w ${lastWhisperer} ` : '/w ';
    case 2:
      return '/s ';
    case 3:
      return '/p ';
    case 4:
      return '/g ';
    default:
      return '';
  }
}
