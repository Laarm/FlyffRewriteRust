// Debug surface (Plan/40 section 13.2): the `window.__flyffDebug` probe. `PerfOverlay.tsx` (`?debug=perf`)
// follows with WP-CLIENT-32.
export {
  createProbe,
  e2eEnabled,
  installProbe,
  type FlyffDebugProbe,
  type ProbeAssets,
  type ProbeBuild,
  type ProbeEntity,
  type ProbeEntityKind,
  type ProbeLoop,
  type ProbeLoopData,
  type ProbeNet,
  type ProbeOptions,
  type ProbeRecoveryEntry,
  type ProbeSelf,
  type ProbeSession,
  type ProbeSources,
  type ProbeTestHooks,
  type TestHookName,
} from './probe';
