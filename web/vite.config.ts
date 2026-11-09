// Vite configuration (Plan/02 section 7.2 normative parts, Plan/40 section 16.1).
import { relative, resolve, sep } from 'node:path';
import react from '@vitejs/plugin-react';
import type { Connect, Plugin } from 'vite';
import { defineConfig } from 'vite';

/** The Rust server (`cargo xtask dev` starts it on this port). */
const SERVER = `http://127.0.0.1:${process.env['FLYFF_DEV_PORT'] ?? '8080'}`;

/**
 * Everything that is not served by Vite itself goes to the Rust server (routes are owned by Plan/24,
 * so this regex never needs to change when routes are added).
 */
const PROXY_KEY =
  '^/(?!$|@|src/|node_modules/|__vite|index\\.html|dev\\.html|admin\\.html|dev/|favicon\\.ico).*';

/** Serve the administration entry point at /admin. */
function rewriteAdminRoute(req: Connect.IncomingMessage, _res: unknown, next: () => void): void {
  const url = req.url ?? '';
  const admin = /^\/admin\/?(\?.*)?$/.exec(url);
  if (admin) req.url = `/admin.html${admin[1] ?? ''}`;
  next();
}

function adminRouteRewrite(): Plugin {
  return {
    name: 'flyff-dev-surfaces-rewrite',
    configureServer(server) {
      server.middlewares.use(rewriteAdminRoute);
    },
    configurePreviewServer(server) {
      server.middlewares.use(rewriteAdminRoute);
    },
  };
}

/**
 * BUILD ONLY: writes `build-modules.json` (output file -> the source modules inside it, relative to `web/`, no
 * dependencies) next to Vite's manifest. `xtask build-web` reads it for the isolation rule of Plan/25 section 11.2
 * (no `src/admin/` module inside any chunk reachable from `index.html`: the manifest alone only names a chunk's entry
 * module, not what Rolldown merged into it) and removes it again after the report.
 */
function buildModuleList(): Plugin {
  return {
    name: 'flyff-build-modules',
    apply: 'build',
    generateBundle(_options, bundle) {
      const root = import.meta.dirname;
      const chunks: Record<string, string[]> = {};
      for (const [file, item] of Object.entries(bundle)) {
        if (item.type !== 'chunk') continue;
        chunks[file] = item.moduleIds
          .filter((id) => !id.includes('node_modules') && !id.startsWith('\0'))
          .map((id) => relative(root, id).split(sep).join('/'));
      }
      this.emitFile({
        type: 'asset',
        fileName: 'build-modules.json',
        source: JSON.stringify(chunks),
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), adminRouteRewrite(), buildModuleList()],
  define: {
    __FLYFF_BUILD_ID__: JSON.stringify(process.env['FLYFF_BUILD_ID'] ?? 'dev'),
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    hmr: { path: '/@vite-hmr' }, // never collides with the server's WebSocket path /ws
    proxy: { [PROXY_KEY]: { target: SERVER, ws: true, changeOrigin: false } },
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    proxy: {}, // `vite preview` serves web/dist only; it must not inherit the dev proxy of server.proxy
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    chunkSizeWarningLimit: 600, // three.js alone is ~520 kB (dev surfaces and, later, the lazy render chunk)
    assetsInlineLimit: 0, // never inline .wasm or fonts
    // Vite's chunk graph, read by `xtask build-web` for the size report and the dev-isolation check, then removed
    manifest: 'build-manifest.json',
    rolldownOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        admin: resolve(import.meta.dirname, 'admin.html'),
      },
      output: {
        codeSplitting: {
          groups: [
            { name: 'react', test: /node_modules[\\/](react|react-dom|scheduler|zustand)[\\/]/ },
            { name: 'three', test: /node_modules[\\/]three[\\/]/ },
            { name: 'proto', test: /node_modules[\\/]@bufbuild[\\/]/ },
          ],
        },
      },
    },
  },
  worker: { format: 'es' },
});
