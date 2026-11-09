// The status bar under the map (Plan/25 section 3.2): the world coordinates under the cursor (a click copies `/te <world> <x> <z>`), the zoom, the
// counts in view per kind, the detail tier the server actually delivers and the update latency; plus what needs saying (too many entities, a refused
// subscription, a pending point pick, the ruler's readout, the heatmap's start).
import { useStore } from 'zustand';
import { copyText } from '../app/clipboard';
import { toast } from '../stores/toasts';
import { sinceLabel } from './heatmap';
import { formatRuler, rulerReadout } from './overlays';
import type { MapSession } from './session';
import { M } from './strings';
import { zoomLabel } from './view';

const nf = new Intl.NumberFormat('en-US');

export function StatusBar({ session }: { readonly session: MapSession }) {
  const s = session.store;
  const cursor = useStore(s, (x) => x.cursor);
  const view = useStore(s, (x) => x.view);
  const counts = useStore(s, (x) => x.counts);
  const tier = useStore(s, (x) => x.tier);
  const degraded = useStore(s, (x) => x.degraded);
  const latency = useStore(s, (x) => x.latencyMs);
  const feed = useStore(s, (x) => x.feed);
  const feedMessage = useStore(s, (x) => x.feedMessage);
  const picking = useStore(s, (x) => x.picking);
  const ruler = useStore(s, (x) => x.ruler);
  const heatSince = useStore(s, (x) => x.heatSince);
  const worldId = useStore(s, (x) => x.worldId);
  const upp = view.upp;
  const copy = (): void => {
    if (!cursor) return;
    const text = M.copyTe(worldId, cursor.x, cursor.z);
    if (copyText(text)) toast.success(M.copied(text));
    else toast.error(new Error('Copying is not available here.'), 'Copy failed');
  };
  return (
    <div className="map-status" role="status" aria-label="Map status">
      <button
        type="button"
        className="map-status__coords"
        title="Copy /te"
        data-testid="map-coords"
        onClick={copy}
        disabled={!cursor}
      >
        {cursor ? `x ${cursor.x.toFixed(1)} z ${cursor.z.toFixed(1)}` : 'x - z -'}
      </button>
      <span data-testid="map-zoom">{`zoom ${zoomLabel(upp)} (${String(Math.round(upp * 100) / 100)} u/px)`}</span>
      <span data-testid="map-counts">
        {`${nf.format(counts.players)} players · ${nf.format(counts.monsters)} monsters · ${nf.format(counts.npcs)} NPCs`}
        {counts.items > 0 ? ` · ${nf.format(counts.items)} items` : ''}
      </span>
      <span data-testid="map-tier">{M.tier(tier)}</span>
      <span data-testid="map-latency">{M.latency(latency)}</span>
      {degraded ? <span className="map-status__warn">{M.degraded}</span> : null}
      {feed === 'refused' ? <span className="map-status__warn">{feedMessage}</span> : null}
      {picking !== null ? (
        <span className="map-status__note">{picking || M.statusPicking}</span>
      ) : null}
      {ruler?.b ? (
        <span className="map-status__note">{formatRuler(rulerReadout(ruler.a, ruler.b))}</span>
      ) : null}
      {heatSince !== null ? <span>{`heatmap ${sinceLabel(heatSince)}`}</span> : null}
    </div>
  );
}
