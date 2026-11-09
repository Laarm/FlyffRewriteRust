// Layering and plain-HTTP rules of the web client (Plan/40 section 2.2, Plan/02 section 11.7).
//
// Exported as flat-config objects so that eslint.config.js (the real lint run) and
// test/lint/layering.test.ts (one fixture per rule, WP-CLIENT-01) use exactly the same rules.
//
// Rules are *replaced*, not merged, when several config objects set the same rule for one file, so the
// `@typescript-eslint/no-restricted-imports` options are computed per area here and the areas are
// disjoint (every source file falls into exactly one block).

import tseslint from 'typescript-eslint';

const REACT = ['react', 'react/**', 'react-dom', 'react-dom/**'];
const THREE = ['three', 'three/**'];
const WASM_PKG = ['**/wasm/pkg/**'];
const WASM_ALL = ['**/wasm/**'];
const RENDER = ['**/render/**'];
const UI = ['**/ui/**'];
const NET = ['**/net/**'];
const NET_SOCKET = ['**/net/socket', '**/net/socket.ts'];
const GAME = ['**/game/**'];
const GAME_SIM_BRIDGE = ['**/game/simBridge', '**/game/simBridge.ts'];
const ASSETS = ['**/assets/**'];
/** The admin console (`src/admin/**`, Plan/25 section 11.2) and the game-only modules it must not reach. */
const ADMIN = ['**/admin', '**/admin/**'];
const BOOT = ['**/app/boot', '**/app/boot.ts'];
const DEV = ['**/dev/**', '**/ui/dev/**'];
/** 41 R15: the visual-only B1 exception may be imported only from `src/render/**` (value AND type). */
const R15 = ['**/render/terrain/heightField', '**/render/objects/groundItems'];
/** The pure modules of src/render (Plan/41 sections 3.1 and 4.3): own rows in `AREAS`. */
const RENDER_PURE = ['src/render/space.ts', 'src/render/rampLut.ts'];
/** Relative imports (the other modules of render/). */
const RELATIVE = ['./*', '../*', './**', '../**'];
/** Every import specifier. */
const ANY_IMPORT = ['*', '**', './*', '../*'];

/**
 * @param {string} message
 * @param {string[]} group
 * @param {boolean} [allowTypeImports]
 */
function pattern(message, group, allowTypeImports = true) {
  return { group, message, allowTypeImports };
}

const MSG = {
  react: 'Layering (Plan/40 2.2): this directory must not import react.',
  three: 'Layering (Plan/40 2.2): this directory must not import three.',
  pkg: 'Layering (Plan/40 CA2): only wasm/, workers/, game/simBridge.ts and rules/ import wasm/pkg.',
  render: 'Layering (Plan/40 2.2): this directory must not import render/**.',
  ui: 'Layering (Plan/40 2.2): this directory must not import ui/**.',
  net: 'Layering (Plan/40 2.2): this directory must not import net/**.',
  socket: 'Layering (Plan/40 2.2): this directory must not import net/socket.',
  game: 'Layering (Plan/40 2.2): this directory must not import game/**.',
  assets: 'Layering (Plan/40 2.2): this directory must not import assets/**.',
  wasm: 'Layering (Plan/40 2.2): this directory must not import wasm/**.',
  bridge: 'Layering (Plan/40 2.2): features use FeatureContext, never game/simBridge.',
  r15: 'Layering (Plan/41 R15): only src/render/** may import the renderer terrain/ground-item helpers.',
  worker:
    'Layering (Plan/40 2.2): decode.worker.ts may import only workers/, wasm/pkg, wasm/decoders and gen/formats.',
  renderSpace:
    'Layering (Plan/41 3.1): render/space.ts may import three.js types only (`import type`) and no other render module.',
  renderRamp: 'Layering (Plan/41 4.3): render/rampLut.ts is a pure module and imports nothing.',
  admin:
    'Layering (Plan/25 11.2): the game and its reused modules never import src/admin/**; the console is a separate entry.',
  adminBoot:
    'Layering (Plan/25 11.2): the console must not import the game boot (app/boot) or game screen state (game/**).',
  adminDev: 'Layering (Plan/25 11.2): the console must not import the dev surfaces.',
};

const p = {
  react: pattern(MSG.react, REACT),
  three: pattern(MSG.three, THREE),
  pkg: pattern(MSG.pkg, WASM_PKG),
  render: pattern(MSG.render, RENDER),
  ui: pattern(MSG.ui, UI),
  net: pattern(MSG.net, NET),
  socket: pattern(MSG.socket, NET_SOCKET),
  game: pattern(MSG.game, GAME),
  assets: pattern(MSG.assets, ASSETS),
  wasm: pattern(MSG.wasm, WASM_ALL),
  bridge: pattern(MSG.bridge, GAME_SIM_BRIDGE),
  r15: pattern(MSG.r15, R15, false),
  admin: pattern(MSG.admin, ADMIN, false),
  adminBoot: pattern(MSG.adminBoot, BOOT, false),
  adminDev: pattern(MSG.adminDev, DEV, false),
};

const EXT = '*.{ts,tsx}';
const TESTS = ['**/*.test.{ts,tsx}', 'test/**', 'src/**/*.typecheck.ts'];

/**
 * Import areas: [name, files, ignores, patterns]. Disjoint by construction (`ignores` removes the
 * more specific areas from the broader ones).
 */
const AREAS = [
  {
    name: 'net+data',
    files: ['src/net/**/' + EXT, 'src/data/**/' + EXT],
    patterns: [p.react, p.three, p.render, p.ui, p.pkg, p.r15, p.admin],
  },
  {
    name: 'stores',
    files: ['src/stores/**/' + EXT],
    ignores: ['src/stores/framework.ts'],
    patterns: [p.react, p.three, p.render, p.ui, p.socket, p.game, p.assets, p.pkg, p.r15, p.admin],
  },
  {
    name: 'stores/framework',
    files: ['src/stores/framework.ts'],
    patterns: [p.three, p.render, p.ui, p.socket, p.game, p.assets, p.pkg, p.r15, p.admin],
  },
  {
    name: 'game/simBridge',
    files: ['src/game/simBridge.ts'],
    patterns: [p.react, p.three, p.render, p.ui, p.r15, p.admin],
  },
  {
    name: 'game',
    files: ['src/game/**/' + EXT],
    ignores: ['src/game/simBridge.ts'],
    patterns: [p.react, p.three, p.render, p.ui, p.pkg, p.r15, p.admin],
  },
  {
    name: 'render',
    files: ['src/render/**/' + EXT],
    // space.ts and rampLut.ts are pure modules with their own rows below
    ignores: RENDER_PURE,
    patterns: [p.react, p.ui, p.net, p.pkg, p.admin],
  },
  {
    // Plan/41 section 3.1: three.js TYPE imports only and no other render/ module, so that the boot path (input,
    // audio) can import `mirrorPos` without pulling in three.js or the lazy render chunk
    name: 'render/space',
    files: ['src/render/space.ts'],
    patterns: [
      pattern(MSG.renderSpace, THREE, true),
      pattern(MSG.renderSpace, RELATIVE, false),
      p.react,
      p.ui,
      p.net,
      p.pkg,
    ],
  },
  {
    // Plan/41 section 4.3: `buildRampLut` is shared with 42's skin/ramp.ts and imports NOTHING
    name: 'render/rampLut',
    files: ['src/render/rampLut.ts'],
    patterns: [pattern(MSG.renderRamp, ANY_IMPORT, false)],
  },
  {
    name: 'ui',
    files: ['src/ui/**/' + EXT],
    patterns: [p.three, p.wasm, p.net, p.r15, p.admin],
  },
  {
    name: 'features',
    files: ['src/features/**/' + EXT],
    patterns: [p.three, p.pkg, p.socket, p.bridge, p.r15, p.admin],
  },
  {
    name: 'decode.worker',
    files: ['src/workers/decode.worker.ts'],
    patterns: [
      p.react,
      p.three,
      pattern(MSG.worker, [
        // gitignore-style negation cannot re-include below an excluded directory, so the forbidden
        // directories are listed (everything except workers/, wasm/pkg, wasm/decoders, gen/formats)
        '../app/**',
        '../platform/**',
        '../auth/**',
        '../net/**',
        '../data/**',
        '../stores/**',
        '../game/**',
        '../features/**',
        '../rules/**',
        '../assets/**',
        '../render/**',
        '../ui/**',
        '../input/**',
        '../audio/**',
        '../debug/**',
        '../dev/**',
        '../admin/**',
        '../styles/**',
        '../static/**',
        '../gen/proto/**',
        '../gen/defs/**',
        '../gen/data/**',
        '../gen/sim/**',
        '../gen/config/**',
        '../wasm/loader',
        '../wasm/call',
        '../wasm/index',
      ]),
      p.r15,
    ],
  },
  {
    name: 'pkg-allowed (wasm, workers, rules)',
    files: ['src/wasm/**/' + EXT, 'src/workers/**/' + EXT, 'src/rules/**/' + EXT],
    ignores: ['src/workers/decode.worker.ts'],
    patterns: [p.r15, p.admin],
  },
  {
    // Plan/25 section 11.2: the console reuses wasm/, workers/, ui/skin/images, ui/icons, features/tooltip and assets/paths
    // (imports of these are fine) but never the game boot, the game's screen state or the dev surfaces
    name: 'admin',
    files: ['src/admin/**/' + EXT],
    patterns: [p.pkg, p.r15, p.game, p.adminBoot, p.adminDev],
  },
];

const AREA_FILES = AREAS.flatMap((a) => a.files);

/** Flat-config objects for `@typescript-eslint/no-restricted-imports`. */
export const layeringConfigs = [
  ...AREAS.map((a) => ({
    name: `flyff/layering/${a.name}`,
    plugins: { '@typescript-eslint': tseslint.plugin },
    files: a.files,
    ...(a.ignores ? { ignores: a.ignores } : {}),
    rules: { '@typescript-eslint/no-restricted-imports': ['error', { patterns: a.patterns }] },
  })),
  {
    // Everything else under src/ (app, platform, assets, input, audio, debug, dev, auth, main.ts)
    name: 'flyff/layering/default',
    plugins: { '@typescript-eslint': tseslint.plugin },
    files: ['src/**/' + EXT],
    ignores: [...AREA_FILES, ...TESTS],
    rules: {
      '@typescript-eslint/no-restricted-imports': ['error', { patterns: [p.pkg, p.r15, p.admin] }],
    },
  },
  {
    name: 'flyff/layering/tests-unrestricted',
    plugins: { '@typescript-eslint': tseslint.plugin },
    files: TESTS,
    rules: { '@typescript-eslint/no-restricted-imports': 'off' },
  },
];

// ---- plain-HTTP rules (D3) and storage / console confinement -----------------------------------

const PLAIN_HTTP_MSG =
  'Plain-HTTP rule (D3, Plan/02 11.7): secure-context-only browser APIs must not be on the critical path.';

const PLAIN_HTTP_PROPERTIES = [
  { object: 'crypto', property: 'subtle', message: PLAIN_HTTP_MSG },
  { object: 'crypto', property: 'randomUUID', message: PLAIN_HTTP_MSG },
  { object: 'navigator', property: 'serviceWorker', message: PLAIN_HTTP_MSG },
  { object: 'navigator', property: 'gpu', message: PLAIN_HTTP_MSG },
  { object: 'navigator', property: 'clipboard', message: PLAIN_HTTP_MSG },
  { object: 'navigator', property: 'mediaDevices', message: PLAIN_HTTP_MSG },
];

const STORAGE_MSG =
  'Storage (Plan/02 11.7): only src/platform/** touches web storage (safeStorage).';
const STORAGE_PROPERTIES = ['window', 'globalThis', 'self'].flatMap((object) =>
  ['localStorage', 'sessionStorage', 'indexedDB'].map((property) => ({
    object,
    property,
    message: STORAGE_MSG,
  })),
);

/** `window.crypto.subtle`, `globalThis.navigator.clipboard`, ... (member chains). */
const CHAIN_SELECTORS = [
  ['crypto', 'subtle'],
  ['crypto', 'randomUUID'],
  ['navigator', 'serviceWorker'],
  ['navigator', 'gpu'],
  ['navigator', 'clipboard'],
  ['navigator', 'mediaDevices'],
].map(([object, property]) => ({
  selector: `MemberExpression[object.property.name='${object}'][property.name='${property}']`,
  message: `${PLAIN_HTTP_MSG} (${object}.${property})`,
}));

const BANNED_GLOBALS = ['caches', 'SharedArrayBuffer', 'Atomics'].map((name) => ({
  name,
  message: PLAIN_HTTP_MSG,
}));
const STORAGE_GLOBALS = ['localStorage', 'sessionStorage'].map((name) => ({
  name,
  message: STORAGE_MSG,
}));

export const plainHttpConfigs = [
  {
    name: 'flyff/plain-http+storage',
    files: ['src/**/' + EXT, 'test/**/' + EXT],
    ignores: ['src/platform/**'],
    rules: {
      'no-restricted-globals': ['error', ...BANNED_GLOBALS, ...STORAGE_GLOBALS],
      'no-restricted-properties': ['error', ...PLAIN_HTTP_PROPERTIES, ...STORAGE_PROPERTIES],
      'no-restricted-syntax': ['error', ...CHAIN_SELECTORS],
      'no-console': 'error',
    },
  },
  {
    // platform/ owns storage (try/catch wrapped); everything else of the plain-HTTP list still applies
    name: 'flyff/plain-http/platform',
    files: ['src/platform/**/' + EXT],
    rules: {
      'no-restricted-globals': ['error', ...BANNED_GLOBALS],
      'no-restricted-properties': ['error', ...PLAIN_HTTP_PROPERTIES],
      'no-restricted-syntax': ['error', ...CHAIN_SELECTORS],
      'no-console': 'error',
    },
  },
  {
    // the logger is the one place that may call console
    name: 'flyff/console/log',
    files: ['src/platform/log.ts'],
    rules: { 'no-console': 'off' },
  },
  {
    // tests and tooling may log
    name: 'flyff/console/tests',
    files: [
      '**/*.test.{ts,tsx}',
      'test/**',
      'e2e/**',
      'vite.config.ts',
      'eslint/**',
      'eslint.config.js',
    ],
    rules: { 'no-console': 'off' },
  },
];
