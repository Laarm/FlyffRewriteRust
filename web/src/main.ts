// Entry point (Plan/40 section 2.1): statically imports only platform/* and the watchdog, installs the
// error hooks, then loads the rest of the client with import() so a failing chunk is reported.
import './styles/fonts.css';
import './styles/reset.css';
import { install as installWatchdog } from './app/watchdog';
import { log } from './platform/log';

installWatchdog();

const root = document.getElementById('root');
if (root) {
  import('./app/boot')
    .then(({ start }) => start(root))
    .catch((e: unknown) => {
      log.error('main', `boot failed: ${e instanceof Error ? e.message : String(e)}`);
      root.textContent = 'Flyff V15 failed to start. See the browser console.';
    });
}
