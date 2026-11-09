// The chat slice of 45 (Plan/45 §2.3, §8): what the chat features remember between messages. The line ring itself is 42's
// `ChatLog` (the single ring, 45 §8.1); this store keeps the last whisperer (`g_Neuz.m_szWhisperName`). The ignore list and the
// privacy flags are the SERVER's since M6 (`messengerStore.settings`, PL-095): the client filters nothing itself. Session scope: it
// survives world changes like the legacy globals and is cleared on logout.

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../stores/registry';

export interface ChatState {
  /** `g_Neuz.m_szWhisperName`: who whispered last (`/r`), `null` before the first whisper. */
  readonly lastWhisperer: string | null;
}

export function initialChatState(): ChatState {
  return { lastWhisperer: null };
}

export type ChatStore = StoreApi<ChatState>;

/** Defines (or returns) the `chat` store of a registry. */
export function defineChatStore(registry: StoreRegistry): ChatStore {
  const existing = registry.get('chat');
  if (existing) return existing as unknown as ChatStore;
  return registry.defineStore<ChatState>({
    name: 'chat',
    scope: 'session',
    owner: '45',
    initial: initialChatState,
  });
}
