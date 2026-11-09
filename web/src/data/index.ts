// Root of src/data/ (Plan/14 section 10, Plan/40 section 3.4). `rowTable.ts` (WP-DATA-18) decodes the column-oriented
// bundle tables and holds the GETTEXT façade; `bundles.ts` (WP-CLIENT-08) is the loader (manifest, boot/lazy/gated
// sets, worker JSON parsing, raw-byte retention, schema check, `ensure`); `clientData.ts` is the typed
// `ClientDataIndex`; `lookup.ts` the lazy row lookups. Layering rules: eslint/rules.js.
export { BUNDLE_FORMAT, BUNDLES } from '../gen/bundles';
export type {
  BundleDataMap,
  BundleEnvelope,
  BundleManifest,
  BundleName,
  LoadClass,
  ManifestEntry,
  RowTable,
} from '../gen/bundles';
export {
  BundleLoader,
  BundleLoadError,
  BundleSchemaError,
  isBundleName,
  manifestFetcher,
  mainThreadBundleParser,
  measureCloneMs,
  PUBLIC_BUNDLES,
  RETAINED_BUNDLES,
  validateManifest,
  workerBundleParser,
  type BundleEnvelopeLike,
  type BundleLoaderOptions,
  type BundleParser,
  type BundleStat,
} from './bundles';
export {
  ClientDataIndex,
  type BundleSource,
  type ClientItem,
  type ClientMover,
  type TextApi,
} from './clientData';
export { RowLookup } from './lookup';
export {
  COLOR_OUT_OF_RANGE,
  TEXT_OUT_OF_RANGE,
  buildTextTable,
  decodeRows,
  getText,
  getTextColor,
  indexRows,
} from './rowTable';
export type { NumericKey, TextTable } from './rowTable';
