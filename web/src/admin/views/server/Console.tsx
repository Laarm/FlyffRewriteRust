// Server -> Console (Plan/25 section 9.9; WP-ADMIN-26): the console of `POST /admin/api/console` with history (Up / Down),
// completion of the console words and the GM command names (Tab), and the `as` selector preset to the acting GM character. GM lines
// run with the authority of the signed-in identity, the console words by the rule of `admin::lines::word_min_authority`: an L staff
// member cannot run a P command.
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react';
import { isApiError, rest } from '../../api/rest';
import { Button, Field, TextInput } from '../../components';
import { safeStorage } from '../../../platform/storage';
import { actingFor, useActing } from '../../stores/acting';
import { useSession } from '../../stores/session';
import { toast } from '../../stores/toasts';
import type { ConsoleReply } from './api';
import { applyCompletion, complete } from './completion';
import { T } from './strings';

const HISTORY_MAX = 100;

/** The history as stored (a JSON list of lines). */
export function loadHistory(): string[] {
  const raw = safeStorage.local.get(T.console.historyKey);
  if (raw === null) return [];
  try {
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v)
      ? v.filter((x): x is string => typeof x === 'string').slice(-HISTORY_MAX)
      : [];
  } catch {
    return [];
  }
}

/** Adds a line to the history (no consecutive duplicates, newest last, bounded). */
export function pushHistory(history: readonly string[], line: string): string[] {
  const t = line.trim();
  if (t === '' || history[history.length - 1] === t) return [...history];
  return [...history, t].slice(-HISTORY_MAX);
}

interface Row {
  readonly id: number;
  readonly kind: 'in' | 'out' | 'err' | 'note';
  readonly text: string;
}

export function Console() {
  const info = useSession((s) => s.info);
  const chosen = useActing((s) => s.chosen);
  const candidates = useActing((s) => s.candidates);
  const loadCandidates = useActing((s) => s.loadCandidates);
  const acting = actingFor(info, chosen, candidates);
  const authority = (info?.authority ?? 'F').charCodeAt(0);
  const [as, setAs] = useState<string | null>(null);
  const [input, setInput] = useState('');
  const [rows, setRows] = useState<readonly Row[]>([{ id: 0, kind: 'note', text: T.console.help }]);
  const [history, setHistory] = useState<string[]>(loadHistory);
  const [cursor, setCursor] = useState<number | null>(null);
  const [hints, setHints] = useState<readonly string[]>([]);
  const [busy, setBusy] = useState(false);
  const next = useRef(1);
  const out = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void loadCandidates();
  }, [loadCandidates]);
  const asName = as ?? acting?.name ?? '';
  const asOptions = useMemo(() => candidates.map((c) => c.name), [candidates]);

  useEffect(() => {
    const el = out.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [rows]);

  const add = (kind: Row['kind'], text: string): void => {
    const id = next.current;
    next.current += 1;
    setRows((r) => [...r.slice(-400), { id, kind, text }]);
  };

  const run = async (): Promise<void> => {
    const line = input.trim();
    if (line === '' || busy) return;
    setBusy(true);
    add('in', asName && !line.startsWith('as ') ? `[as ${asName}] ${line}` : line);
    const h = pushHistory(history, line);
    setHistory(h);
    safeStorage.local.set(T.console.historyKey, JSON.stringify(h));
    setCursor(null);
    setInput('');
    setHints([]);
    try {
      const r = await rest.post<ConsoleReply>('/admin/api/console', {
        line,
        ...(asName ? { as: asName } : {}),
      });
      for (const l of r.lines) add(r.result === 'ok' ? 'out' : 'err', l);
      if (r.lines.length === 0 && r.result === 'ok') add('note', 'ok');
    } catch (e) {
      if (isApiError(e, 'audit_unavailable')) add('err', `${e.message} (nothing was run)`);
      else toast.error(e, 'Console');
    } finally {
      setBusy(false);
    }
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === 'Enter') {
      e.preventDefault();
      void run();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (history.length === 0) return;
      const i = cursor === null ? history.length - 1 : Math.max(0, cursor - 1);
      setCursor(i);
      setInput(history[i] ?? '');
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (cursor === null) return;
      const i = cursor + 1;
      if (i >= history.length) {
        setCursor(null);
        setInput('');
      } else {
        setCursor(i);
        setInput(history[i] ?? '');
      }
    } else if (e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      const c = complete(input, authority);
      setHints(c.candidates.length > 1 ? c.candidates.slice(0, 40) : []);
      setInput(applyCompletion(input, c));
    }
  };

  return (
    <div className="adm-stack adm-console">
      <p className="adm-muted">
        {T.console.authority(info?.identity ?? 'unknown', info?.authority ?? '?')}
      </p>
      <div
        ref={out}
        className="adm-logbox adm-console__out"
        role="log"
        aria-live="polite"
        aria-label="Console output"
      >
        {rows.map((r) => (
          <div key={r.id} className={`adm-console__row adm-console__row--${r.kind}`}>
            {r.kind === 'in' ? `> ${r.text}` : r.text}
          </div>
        ))}
      </div>
      {hints.length > 0 && (
        <p className="adm-muted adm-small" aria-label="Completions">
          {hints.join('  ')}
        </p>
      )}
      <div className="adm-row">
        <div className="adm-grow">
          <TextInput
            aria-label="Console command"
            className="adm-mono"
            placeholder={T.console.placeholder}
            value={input}
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => {
              setInput(e.target.value);
              setCursor(null);
            }}
            onKeyDown={onKey}
          />
        </div>
        <Button
          variant="primary"
          loading={busy}
          onClick={() => void run()}
          disabled={input.trim() === ''}
        >
          {T.console.run}
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setRows([]);
          }}
        >
          {T.console.clear}
        </Button>
      </div>
      <Field
        label={T.console.as}
        hint={acting === null && as === null ? undefined : T.console.asHint}
      >
        <TextInput
          list="adm-console-as"
          value={asName}
          placeholder="(none)"
          onChange={(e) => {
            setAs(e.target.value);
          }}
        />
      </Field>
      <datalist id="adm-console-as">
        {asOptions.map((n) => (
          <option key={n} value={n} />
        ))}
      </datalist>
    </div>
  );
}
