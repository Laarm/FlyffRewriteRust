// The Live World map (Plan/25 section 3, WP-ADMIN-09): the lazily loaded view of the Map tab. It owns nothing that lives longer than the tab: the
// runtime (canvases, engine, controller) is acquired when the view mounts and parked when it unmounts (the GPU textures are freed two minutes
// later, `runtime.ts`), the subscription closes the moment the tab is left.
import { Suspense, useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { Route } from '../app/route';
import { useMediaQuery } from '../app/useMediaQuery';
import { Button } from '../components/Button';
import { ErrorState } from '../components/EmptyState';
import { Skeleton } from '../components/Spinner';
import { useLive } from '../stores/live';
import { useLayerFeed, useLayers } from '../views/world/layers';
import type { MapSlots } from './api';
import { attachInteraction } from './interaction';
import { LocationsPanel } from './LocationsPanel';
import { MinimapView } from './MinimapView';
import { acquireRuntime, releaseRuntime, type MapRuntime } from './runtime';
import { Sidebar } from './Sidebar';
import { createMapStore } from './state';
import { StatusBar } from './StatusBar';
import { M } from './strings';
import { Toolbar } from './Toolbar';
import './map.css';

/** What the view reads while there is no runtime yet. */
const EMPTY_STORE = createMapStore();

export interface MapViewProps {
  readonly route: Route;
  readonly slots?: MapSlots;
}

/** A scale bar of a round length between 60 and 140 px, and the north arrow (north = +z = up). */
function ScaleBar({ upp }: { readonly upp: number }) {
  const steps = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
  const metres = steps.find((m) => m / upp >= 60) ?? 20000;
  const px = metres / upp;
  return (
    <div className="map-scale" aria-hidden="true">
      <div className="map-scale__bar" style={{ width: px }} />
      <span>{metres >= 1000 ? `${String(metres / 1000)} km` : `${String(metres)} m`}</span>
    </div>
  );
}

export default function MapView({ route, slots = {} }: MapViewProps) {
  const [rt, setRt] = useState<MapRuntime | null>(null);
  const stageHost = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const findRef = useRef<HTMLInputElement>(null);
  const epoch = useLive((s) => s.epoch);
  const narrow = useMediaQuery('(max-width: 1439px)');
  const [rail, setRail] = useState<boolean | null>(null);
  useLayerFeed();

  // acquire, attach, start; on leaving park the runtime (the subscription closes at once)
  useEffect(() => {
    const host = stageHost.current;
    if (!host) return;
    const runtime = acquireRuntime();
    runtime.attach(host);
    setRt(runtime);
    return () => {
      releaseRuntime(runtime);
      setRt(null);
    };
  }, []);

  // the first view: the route, the last view of this browser, or Flaris; a route that changes under the map is followed
  const routeParams = route.params;
  const started = useRef<MapRuntime | null>(null);
  useEffect(() => {
    if (!rt) return;
    if (started.current !== rt) {
      started.current = rt;
      if (rt.session.started) void rt.session.applyRoute(routeParams);
      else void rt.session.start(routeParams);
    } else {
      void rt.session.applyRoute(routeParams);
    }
  }, [rt, routeParams]);

  // pointer and keyboard
  useEffect(() => {
    if (!rt) return;
    const host = rt.root;
    return attachInteraction({
      host,
      engine: rt.engine,
      session: rt.session,
      focusFind: () => {
        findRef.current?.focus();
        findRef.current?.select();
      },
    });
  }, [rt]);

  // a new socket (a sign-in as staff): the streams are opened again
  const firstEpoch = useRef(epoch);
  useEffect(() => {
    if (!rt || firstEpoch.current === epoch) return;
    firstEpoch.current = epoch;
    rt.session.reopen();
  }, [rt, epoch]);

  // a layer that closed while it is open
  const layers = useLayers((s) => s.layers);
  useEffect(() => {
    rt?.session.layersChanged(layers);
  }, [rt, layers]);

  const store = rt?.session.store ?? EMPTY_STORE;
  const phase = useStore(store, (s) => s.phase);
  const error = useStore(store, (s) => s.error);
  const paused = useStore(store, (s) => s.paused);
  const sidebarOpen = useStore(store, (s) => s.sidebarOpen);
  const layerClosed = useStore(store, (s) => s.layerClosed);
  const picking = useStore(store, (s) => s.picking);
  const refused = useStore(store, (s) => s.feed) === 'refused';
  const feedMessage = useStore(store, (s) => s.feedMessage);
  const upp = useStore(store, (s) => s.view.upp);
  const noTiles = useStore(store, (s) => s.meta !== null && s.meta.tiles === null);
  // layer 0 of a world made of instances is the empty template layer (Plan/25 section 3.13)
  const templateLayer = useStore(
    store,
    (s) => s.meta?.layer_policy === 'pick_live' && s.layer === 0 && s.phase === 'ready',
  );
  const showRail = rail ?? narrow;

  const Locations = slots.locations;
  const Overlay = slots.overlay;
  return (
    <div
      className="map"
      data-testid="map"
      data-sidebar={sidebarOpen ? 'open' : 'closed'}
      data-locations={showRail ? 'rail' : 'full'}
      ref={rootRef}
    >
      {rt ? (
        <Toolbar session={rt.session} fullscreenRef={rootRef} findRef={findRef} />
      ) : (
        <div className="map-toolbar" />
      )}
      <div className="map__body">
        <aside className="map__locations" aria-label="Locations">
          {rt ? (
            Locations ? (
              <Locations
                api={rt.session}
                rail={showRail}
                onToggleRail={() => {
                  setRail(!showRail);
                }}
              />
            ) : (
              <LocationsPanel
                session={rt.session}
                rail={showRail}
                onToggleRail={() => {
                  setRail(!showRail);
                }}
              />
            )
          ) : null}
        </aside>
        <div className="map__stagewrap">
          <div className="map__stagehost" ref={stageHost} />
          {rt && phase === 'ready' ? (
            <>
              <MinimapView session={rt.session} />
              <ScaleBar upp={upp} />
              <div className="map-north" aria-hidden="true">
                N<span>↑</span>
              </div>
            </>
          ) : null}
          {phase === 'loading' ? (
            <div className="map-cover">
              <Skeleton lines={4} height={18} />
              <p>{M.loading}</p>
            </div>
          ) : null}
          {phase === 'error' ? (
            <div className="map-cover">
              <ErrorState
                title={M.loadFailed}
                error={error ?? undefined}
                onRetry={() => {
                  void rt?.session.start(routeParams);
                }}
              />
            </div>
          ) : null}
          {paused ? (
            <div className="map-cover map-cover--paused" data-testid="map-paused">
              <h3>{M.paused}</h3>
              <p>{M.pausedHint}</p>
              <Button
                variant="primary"
                onClick={() => {
                  rt?.session.resume();
                }}
              >
                {M.resume}
              </Button>
            </div>
          ) : null}
          <div className="map-banners">
            {noTiles ? <div className="map-banner map-banner--info">{M.noTiles}</div> : null}
            {templateLayer ? (
              <div className="map-banner map-banner--info" data-testid="template-layer">
                {M.templateLayer}
              </div>
            ) : null}
            {layerClosed ? (
              <div className="map-banner map-banner--warn" role="alert">
                {M.layerClosed}{' '}
                <Button
                  size="sm"
                  onClick={() => {
                    void rt?.session.setLayer(0);
                  }}
                >
                  {M.layerClosedAction}
                </Button>
              </div>
            ) : null}
            {refused && !layerClosed ? (
              <div className="map-banner map-banner--warn" role="alert">
                {feedMessage}
              </div>
            ) : null}
            {picking !== null ? <div className="map-banner map-banner--info">{picking}</div> : null}
          </div>
          {rt && Overlay ? <Overlay api={rt.session} /> : null}
          {rt ? (
            <button
              type="button"
              className="map-sidebar-toggle"
              aria-label={sidebarOpen ? M.sidebarHide : M.sidebarShow}
              title={sidebarOpen ? M.sidebarHide : M.sidebarShow}
              onClick={() => {
                rt.session.setSidebarOpen(!sidebarOpen);
              }}
            >
              {sidebarOpen ? '›' : '‹'}
            </button>
          ) : null}
        </div>
        {sidebarOpen ? (
          <aside className="map__sidebar" aria-label="Map sidebar">
            {rt ? (
              <Suspense fallback={<Skeleton lines={4} />}>
                <Sidebar session={rt.session} slots={slots} />
              </Suspense>
            ) : null}
          </aside>
        ) : null}
      </div>
      {rt ? <StatusBar session={rt.session} /> : <div className="map-status" />}
    </div>
  );
}
