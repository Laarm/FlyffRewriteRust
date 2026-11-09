// A small Markdown reader for the operator guide shown inside the console (`docs/admin-console.md`, WP-ADMIN-32 "in-app ? links to the
// guide's anchors"). It reads exactly the subset the guide uses: headings, paragraphs, bullet and numbered lists (continuation lines
// indented), pipe tables, fenced code, block quotes, images, and inline code, bold, italic and links. A line that is only
// `<a id="name"></a>` gives the next heading its anchor. Nothing is ever inserted as HTML: the result is a tree that `Guide.tsx` turns
// into elements, so a guide with markup in it cannot inject any.

export type Inline =
  | { readonly t: 'text'; readonly text: string }
  | { readonly t: 'code'; readonly text: string }
  | { readonly t: 'strong'; readonly children: readonly Inline[] }
  | { readonly t: 'em'; readonly children: readonly Inline[] }
  | { readonly t: 'link'; readonly href: string; readonly children: readonly Inline[] }
  | { readonly t: 'image'; readonly alt: string; readonly src: string };

export type Block =
  | {
      readonly t: 'heading';
      readonly level: number;
      readonly id: string;
      readonly children: readonly Inline[];
    }
  | { readonly t: 'p'; readonly children: readonly Inline[] }
  | {
      readonly t: 'list';
      readonly ordered: boolean;
      readonly items: readonly (readonly Inline[])[];
    }
  | { readonly t: 'code'; readonly lang: string; readonly text: string }
  | {
      readonly t: 'table';
      readonly head: readonly (readonly Inline[])[];
      readonly rows: readonly (readonly (readonly Inline[])[])[];
    }
  | { readonly t: 'quote'; readonly blocks: readonly Block[] }
  | { readonly t: 'rule' };

/** The id GitHub would give a heading without an explicit anchor. */
export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/`/g, '')
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-');
}

const INLINE =
  /!\[([^\]]*)\]\(([^)\s]+)\)|\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*|\*([^*]+)\*/g;

export function parseInline(text: string): Inline[] {
  const out: Inline[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) out.push({ t: 'text', text: text.slice(last, m.index) });
    if (m[2] !== undefined) out.push({ t: 'image', alt: m[1] ?? '', src: m[2] });
    else if (m[4] !== undefined)
      out.push({ t: 'link', href: m[4], children: parseInline(m[3] ?? '') });
    else if (m[5] !== undefined) out.push({ t: 'code', text: m[5] });
    else if (m[6] !== undefined) out.push({ t: 'strong', children: parseInline(m[6]) });
    else if (m[7] !== undefined) out.push({ t: 'em', children: parseInline(m[7]) });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ t: 'text', text: text.slice(last) });
  return out;
}

const ANCHOR = /^<a id="([^"]+)"><\/a>\s*$/;
const HEADING = /^(#{1,4})\s+(.*?)\s*$/;
const FENCE = /^```(.*)$/;
const TABLE_ROW = /^\s*\|.*\|\s*$/;
const TABLE_RULE = /^\s*\|[\s:|-]+\|\s*$/;
const LIST_ITEM = /^(\s*)([-*]|\d+\.)\s+(.*)$/;

function cells(row: string): string[] {
  return row
    .trim()
    .replace(/^\||\|$/g, '')
    .split('|')
    .map((c) => c.trim());
}

function startsBlock(line: string): boolean {
  return (
    HEADING.test(line) ||
    FENCE.test(line) ||
    ANCHOR.test(line) ||
    line.startsWith('>') ||
    LIST_ITEM.test(line) ||
    TABLE_ROW.test(line) ||
    /^---+\s*$/.test(line)
  );
}

export function parseMarkdown(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let anchor: string | null = null;
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? '';
    if (line.trim() === '') {
      i += 1;
      continue;
    }
    const a = ANCHOR.exec(line);
    if (a) {
      anchor = a[1] ?? null;
      i += 1;
      continue;
    }
    const fence = FENCE.exec(line);
    if (fence) {
      const body: string[] = [];
      i += 1;
      while (i < lines.length && !(lines[i] ?? '').startsWith('```')) {
        body.push(lines[i] ?? '');
        i += 1;
      }
      i += 1;
      blocks.push({ t: 'code', lang: (fence[1] ?? '').trim(), text: body.join('\n') });
      continue;
    }
    const h = HEADING.exec(line);
    if (h) {
      const text = h[2] ?? '';
      blocks.push({
        t: 'heading',
        level: (h[1] ?? '#').length,
        id: anchor ?? slugify(text),
        children: parseInline(text),
      });
      anchor = null;
      i += 1;
      continue;
    }
    if (/^---+\s*$/.test(line)) {
      blocks.push({ t: 'rule' });
      i += 1;
      continue;
    }
    if (line.startsWith('>')) {
      const inner: string[] = [];
      while (i < lines.length && (lines[i] ?? '').startsWith('>')) {
        inner.push((lines[i] ?? '').replace(/^>\s?/, ''));
        i += 1;
      }
      blocks.push({ t: 'quote', blocks: parseMarkdown(inner.join('\n')) });
      continue;
    }
    if (TABLE_ROW.test(line) && TABLE_RULE.test(lines[i + 1] ?? '')) {
      const head = cells(line).map(parseInline);
      i += 2;
      const rows: Inline[][][] = [];
      while (i < lines.length && TABLE_ROW.test(lines[i] ?? '')) {
        rows.push(cells(lines[i] ?? '').map(parseInline));
        i += 1;
      }
      blocks.push({ t: 'table', head, rows });
      continue;
    }
    const li = LIST_ITEM.exec(line);
    if (li) {
      const ordered = /\d/.test(li[2] ?? '');
      const items: string[] = [];
      while (i < lines.length) {
        const cur = lines[i] ?? '';
        const m = LIST_ITEM.exec(cur);
        if (m && (m[1] ?? '').length === (li[1] ?? '').length) {
          items.push(m[3] ?? '');
        } else if (cur.trim() !== '' && /^\s+/.test(cur) && items.length > 0) {
          const last = items.length - 1;
          items[last] = `${items[last] ?? ''} ${cur.trim()}`; // a continuation line
        } else {
          break;
        }
        i += 1;
      }
      blocks.push({ t: 'list', ordered, items: items.map(parseInline) });
      continue;
    }
    const para: string[] = [line];
    i += 1;
    while (i < lines.length && (lines[i] ?? '').trim() !== '' && !startsBlock(lines[i] ?? '')) {
      para.push(lines[i] ?? '');
      i += 1;
    }
    blocks.push({ t: 'p', children: parseInline(para.join(' ')) });
  }
  return blocks;
}

/** The plain text of inline nodes (headings in a table of contents). */
export function inlineText(nodes: readonly Inline[]): string {
  return nodes
    .map((n) => {
      switch (n.t) {
        case 'text':
        case 'code':
          return n.text;
        case 'image':
          return n.alt;
        case 'strong':
        case 'em':
        case 'link':
          return inlineText(n.children);
      }
    })
    .join('');
}
