// Ports of the job window shells (Plan/42 §19.4 D, WP-UI-38; wired by 45 WP-CFEAT-33): the fast job change window and the two job
// transfer windows of the class-change scroll. Each window is opened with ONE request object: the data it shows and the callbacks
// of its buttons. What the callbacks send (`UpdateJob`) and when the windows open is 45's; the shells keep the control behaviour
// of the originals (`src:_Interface/WndChangeJobEx.cpp`, `WndField.cpp:13440-13760`).

/**
 * `CWndJobChangeEx` (`APP_FASTJOBCHANGE`, `features.fast_jobchange`): the classes the character may change to, one at a time.
 * `targets` is `rules.fastJobTargets(level, job)` (the strict table of Plan/16 §11.4); an empty list is the error state of the
 * original ("Error", every button disabled).
 */
export interface FastJobRequest {
  /** The jobs on offer, ascending (`nJobMin..nJobMax`); empty = no class can be chosen at this level. */
  readonly targets: readonly number[];
  /** The name of a job (`prj.m_aJob[job].szName`). */
  jobName(job: number): string;
  /** The legacy class text of a job (`szInfo[...]`, `features/jobchange/texts.ts`). */
  description(job: number): string;
  /** `WIDC_BUTTON1` ("Select Class"): `UpdateJob{job}`; the window closes. */
  apply(job: number): void;
  closed?(): void;
}

/**
 * `CWndChangeClass1` (`APP_CHANGECLASS_1`, first-job characters) and `CWndChangeClass2` (`APP_CHANGECLASS_2`, second-job
 * characters), opened by the class-change scroll. The own job is pre-checked and its radio disabled.
 */
export interface ChangeClassRequest {
  /** `g_pPlayer->GetJob()`. */
  readonly job: number;
  /** OK on another job: the legacy `SendChangeJob(job, FALSE)`; the port sends nothing (the server handler does not exist), the window closes. */
  select(job: number): void;
  /** OK on the own job: the line `TID_GAME_EQUALJOB` (the window stays open). */
  equalJob(): void;
  closed?(): void;
}
