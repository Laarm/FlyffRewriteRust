// ESLint flat config (Plan/02 section 11.7, Plan/40 section 2.2).
import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import { layeringConfigs, plainHttpConfigs } from './eslint/rules.js';

export default defineConfig(
  // generated code and build output are never linted
  globalIgnores([
    'dist/**',
    'node_modules/**',
    'src/gen/**',
    'src/admin/gen/**',
    'src/wasm/pkg/**',
    'src/wasm/pkg-testkit/**',
    'src/wasm/pkg-testkit.tmp/**',
    'src/wasm/pkg-testkit.hidden/**',
    'src/static/**',
    'test-results/**',
    'playwright-report/**',
    'e2e/.local-baselines/**',
  ]),
  js.configs.recommended,
  tseslint.configs.strictTypeChecked,
  tseslint.configs.stylisticTypeChecked,
  {
    languageOptions: {
      ecmaVersion: 2023,
      globals: { ...globals.browser, ...globals.es2021 },
      parserOptions: {
        project: ['./tsconfig.app.json', './tsconfig.worker.json', './tsconfig.node.json'],
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-non-null-assertion': 'error',
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/switch-exhaustiveness-check': 'error',
      '@typescript-eslint/consistent-type-imports': [
        'error',
        { prefer: 'type-imports', fixStyle: 'inline-type-imports' },
      ],
      '@typescript-eslint/restrict-template-expressions': [
        'error',
        { allowNumber: true, allowBoolean: true },
      ],
    },
  },
  {
    files: ['**/*.{ts,tsx}'],
    extends: [reactHooks.configs.flat['recommended-latest']],
  },
  // layering + plain-HTTP + storage/console confinement (eslint/rules.js)
  ...layeringConfigs,
  ...plainHttpConfigs,
  {
    // Node-side files: configuration, e2e specs, the lint tables themselves
    files: ['vite.config.ts', 'eslint/**/*.js', 'eslint.config.js'],
    languageOptions: { globals: { ...globals.node } },
  },
  {
    // plain JavaScript is not type-checked
    files: ['**/*.{js,mjs,cjs}'],
    extends: [tseslint.configs.disableTypeChecked],
  },
);
