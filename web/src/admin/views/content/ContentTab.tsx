// World & Content -> Content (Plan/25 section 7.3): the panels of the later systems (Event.lua, lord, tax, secret room, guild siege, rainbow race,
// quiz, instance dungeons, housing) from `content/registry.ts`. Each panel says what it will show and offer and which work package it waits for;
// once a panel's system exists its view is registered there and shows here.
import { useState } from 'react';
import { NeedsFeature } from '../../components';
import { CONTENT_PANELS, panelView, type ContentPanel } from '../../content/registry';
import { T } from './strings';

const K = T.content;

function PanelBody({ panel }: { readonly panel: ContentPanel }) {
  const draw = panelView(panel.id);
  if (draw !== undefined) return <>{draw()}</>;
  return (
    <div className="content-panel">
      <NeedsFeature wp={panel.wp} title={K.soon} />
      <dl className="content-facts">
        <dt>{K.reads}</dt>
        <dd>{panel.reads}</dd>
        <dt>{K.actions}</dt>
        <dd>
          {panel.actions.length === 0 ? (
            K.readOnly
          ) : (
            <ul>
              {panel.actions.map((a) => (
                <li key={a}>{a}</li>
              ))}
            </ul>
          )}
        </dd>
      </dl>
    </div>
  );
}

export function ContentTab() {
  const [id, setId] = useState<string>(CONTENT_PANELS[0]?.id ?? '');
  const panel = CONTENT_PANELS.find((p) => p.id === id) ?? CONTENT_PANELS[0];
  return (
    <div className="content-tab">
      <p className="adm-muted">{K.hint}</p>
      <div className="content-panels">
        <nav aria-label={K.select}>
          <ul className="content-nav">
            {CONTENT_PANELS.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  className={`content-nav__item ${p.id === id ? 'is-selected' : ''}`}
                  aria-current={p.id === id ? 'page' : undefined}
                  onClick={() => {
                    setId(p.id);
                  }}
                >
                  {p.title}
                  {panelView(p.id) === undefined && <span className="adm-muted"> ({p.wp})</span>}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <section aria-label={panel?.title ?? K.title}>
          {panel && (
            <>
              <h3 className="content-h">{panel.title}</h3>
              <PanelBody panel={panel} />
            </>
          )}
        </section>
      </div>
    </div>
  );
}
