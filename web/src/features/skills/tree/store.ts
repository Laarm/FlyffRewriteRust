// `skillTreeStore` (Plan/45 §2.3, WP-CFEAT-31): the local editable copy of the 45 job-skill slots, the local skill points, the
// skill points and the table the server knows, the selected slot and the "finish in flight" flag. Writers: `SpawnSelf`,
// `JobChanged`, `DoUseSkillPointNtf`, `InitSkillPoint`, `CmdSetSkillLevel`, `SetExperience` (`reducers.ts`). Scope `session`: the
// table is part of the character that joined and is sent again with every `SpawnSelf`.

import type { StoreApi } from 'zustand/vanilla';
import type { StoreRegistry } from '../../../stores/registry';
import { initState, type TreeState } from './model';

export interface SkillTreeState {
  /** The server table, the local copy, both skill-point counts and the selected slot. */
  readonly tree: TreeState;
  /** A table has been received (`SpawnSelf` carried the 45 slots). */
  readonly ready: boolean;
  /** `DoUseSkillPointReq` was sent and `DoUseSkillPointNtf` has not come back: the buttons wait (the request gate of 45 §2.4). */
  readonly busy: boolean;
  /** Bumps on every change of the server table (the view and the tooltips cache by it). */
  readonly serverVersion: number;
}

export type SkillTreeStore = StoreApi<SkillTreeState>;

export function initialSkillTreeState(): SkillTreeState {
  return { tree: initState([], 0), ready: false, busy: false, serverVersion: 0 };
}

/** Defines (or finds) the store of a registry. */
export function defineSkillTreeStore(registry: StoreRegistry): SkillTreeStore {
  const existing = registry.get('skillTree');
  if (existing) return existing as unknown as SkillTreeStore;
  return registry.defineStore<SkillTreeState>({
    name: 'skillTree',
    scope: 'session',
    owner: '45',
    initial: initialSkillTreeState,
  });
}
