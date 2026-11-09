// The operator guide inside the console: `docs/admin-console.md` rendered in a dialog, opened at the anchor of a category from its "?" button
// (Plan/25 WP-ADMIN-32). It is a lazy chunk (the markdown and the screenshots are bundled with it), so the console's entry does not pay for it.
import { useEffect, useMemo, useRef, type ReactNode } from 'react';
import guideSource from '../../../../docs/admin-console.md?raw';
import { Dialog } from '../components/Dialog';
import { parseMarkdown, inlineText, type Block, type Inline } from './markdown';

/** The screenshots of the guide, by file name (`admin-guide-dashboard.png`), as URLs of the build. */
const SHOTS = import.meta.glob<string>('../../../../docs/shots/admin-guide-*.png', {
  query: '?url',
  import: 'default',
  eager: true,
});

function shotUrl(src: string): string | null {
  const name = src.split('/').pop() ?? '';
  const key = Object.keys(SHOTS).find((k) => k.endsWith(`/${name}`));
  return key ? (SHOTS[key] ?? null) : null;
}

function renderInline(nodes: readonly Inline[]): ReactNode[] {
  return nodes.map((n, i) => {
    switch (n.t) {
      case 'text':
        return n.text;
      case 'code':
        return <code key={i}>{n.text}</code>;
      case 'strong':
        return <strong key={i}>{renderInline(n.children)}</strong>;
      case 'em':
        return <em key={i}>{renderInline(n.children)}</em>;
      case 'image': {
        const url = shotUrl(n.src);
        // a screenshot that is not part of this build shows its description
        return url ? (
          <img key={i} className="adm-guide__shot" src={url} alt={n.alt} loading="lazy" />
        ) : (
          <em key={i}>[{n.alt}]</em>
        );
      }
      case 'link':
        if (n.href.startsWith('#')) {
          // handled by the click handler of the guide (it scrolls inside the dialog)
          return (
            <a key={i} href={n.href} data-guide-anchor={n.href.slice(1)}>
              {renderInline(n.children)}
            </a>
          );
        }
        if (/^https?:\/\//.test(n.href)) {
          return (
            <a key={i} href={n.href} target="_blank" rel="noopener noreferrer">
              {renderInline(n.children)}
            </a>
          );
        }
        // a file of the repository: shown, not linked (the console cannot open it)
        return <code key={i}>{inlineText(n.children)}</code>;
    }
  });
}

function renderBlock(b: Block, key: number): ReactNode {
  switch (b.t) {
    case 'heading': {
      const Tag = `h${String(Math.min(6, b.level + 1))}` as 'h2';
      return (
        <Tag key={key} id={`guide-${b.id}`}>
          {renderInline(b.children)}
        </Tag>
      );
    }
    case 'p':
      return <p key={key}>{renderInline(b.children)}</p>;
    case 'list': {
      const Tag = b.ordered ? 'ol' : 'ul';
      return (
        <Tag key={key}>
          {b.items.map((item, i) => (
            <li key={i}>{renderInline(item)}</li>
          ))}
        </Tag>
      );
    }
    case 'code':
      return (
        <pre key={key}>
          <code>{b.text}</code>
        </pre>
      );
    case 'table':
      return (
        <table key={key} className="adm-plain">
          <thead>
            <tr>
              {b.head.map((c, i) => (
                <th key={i}>{renderInline(c)}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {b.rows.map((r, i) => (
              <tr key={i}>
                {r.map((c, j) => (
                  <td key={j}>{renderInline(c)}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      );
    case 'quote':
      return <blockquote key={key}>{b.blocks.map((x, i) => renderBlock(x, i))}</blockquote>;
    case 'rule':
      return <hr key={key} />;
  }
}

export interface GuideProps {
  /** The anchor to scroll to (`players`, `remote-access`). */
  readonly anchor?: string;
  readonly onClose: () => void;
  /** The markdown (tests inject their own; the build bundles `docs/admin-console.md`). */
  readonly source?: string;
}

export function GuideDialog({ anchor, onClose, source = guideSource }: GuideProps) {
  const blocks = useMemo(() => parseMarkdown(source), [source]);
  const body = useRef<HTMLDivElement>(null);
  const go = (id: string): void => {
    body.current
      ?.querySelector<HTMLElement>(`[id="guide-${CSS.escape(id)}"]`)
      ?.scrollIntoView({ block: 'start' });
  };
  useEffect(() => {
    if (anchor) {
      body.current
        ?.querySelector<HTMLElement>(`[id="guide-${CSS.escape(anchor)}"]`)
        ?.scrollIntoView({ block: 'start' });
    }
  }, [anchor]);
  const sections = blocks.filter(
    (b): b is Extract<Block, { t: 'heading' }> => b.t === 'heading' && b.level === 2,
  );
  return (
    <Dialog title="Operator guide" onClose={onClose} size="lg" dismissOnOverlay>
      {/* a click on an in-page link (the contents, a link of the text) scrolls inside the dialog */}
      <div
        ref={body}
        className="adm-guide"
        onClick={(e) => {
          const link = (e.target as HTMLElement).closest<HTMLElement>('a[data-guide-anchor]');
          if (link?.dataset['guideAnchor']) {
            e.preventDefault();
            go(link.dataset['guideAnchor']);
          }
        }}
      >
        <nav aria-label="Guide sections" className="adm-guide__toc">
          {sections.map((s) => (
            <a key={s.id} href={`#${s.id}`} data-guide-anchor={s.id}>
              {inlineText(s.children)}
            </a>
          ))}
        </nav>
        {blocks.map((b, i) => renderBlock(b, i))}
      </div>
    </Dialog>
  );
}

export default GuideDialog;
