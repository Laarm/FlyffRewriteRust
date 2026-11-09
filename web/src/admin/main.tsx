// Entry of the admin console (`web/admin.html`, Plan/25 section 11.2): a third Vite entry next to the game (`index.html`) and the dev
// surfaces (`dev.html`). Nothing of it is reachable from `index.html`, and it imports no game screen state (`xtask build-web` and
// eslint/rules.js enforce both). The shell loads each category lazily; the map engine loads only when the Map tab opens.
import { createRoot } from 'react-dom/client';
import './styles/index.css';
import { App } from './app/App';
import { ErrorBoundary } from './app/ErrorBoundary';
import { applyStoredUi } from './stores/ui';

applyStoredUi();

const root = document.getElementById('admin-root');
if (root) {
  createRoot(root).render(
    <ErrorBoundary>
      <App />
    </ErrorBoundary>,
  );
}
