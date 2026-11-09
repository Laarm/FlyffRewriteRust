// Server -> Logs (Plan/25 section 9.8; WP-ADMIN-26): the server log as a live tail. The server keeps the last 10,000 lines in a ring
// buffer (`admin::logs`) and streams them over the admin socket (`LogSubscribe` / `LogLine`, `act.server`): the tail on subscribe,
// then every new line. Filters: level and target prefix (on the server), text (here). Pause, copy and download the visible lines.
import { useEffect, useMemo, useRef, useState } from 'react';
import { copyText } from '../../app/clipboard';
import { Button, Field, Select, TextInput } from '../../components';
import { adminSocket, useLive } from '../../stores/live';
import { toast } from '../../stores/toasts';
import {
  appendLines,
  formatLogLine,
  LEVELS,
  levelName,
  matchesText,
  visibleText,
  type LogEntry,
} from './logStream';
import { T } from './strings';

interface Stream {
  readonly lines: readonly LogEntry[];
  readonly note: string | null;
  readonly clear: () => void;
}

/** Subscribes to the log while mounted; lines are batched every 150 ms so a flood does not render per line. */
export function useLogStream(minLevel: number, prefix: string): Stream {
  const epoch = useLive((s) => s.epoch);
  const sig = `${String(minLevel)}|${prefix}|${String(epoch)}`;
  // the lines belong to the subscription that produced them: a new level, prefix or socket starts an empty view
  const [state, setState] = useState<{
    sig: string;
    lines: readonly LogEntry[];
    note: string | null;
  }>({ sig, lines: [], note: null });
  const pending = useRef<LogEntry[]>([]);
  const counter = useRef(0);

  useEffect(() => {
    pending.current = [];
    const sub = adminSocket().subscribeLog(
      minLevel,
      prefix,
      (l) => {
        counter.current += 1;
        pending.current.push({
          key: counter.current,
          ts_ms: Number(l.tsMs),
          level: l.level,
          target: l.target,
          text: l.text,
        });
      },
      {
        onError: () => {
          setState((s) => ({ sig, lines: s.sig === sig ? s.lines : [], note: T.logs.refused }));
        },
        onResync: () => {
          setState((s) => ({ sig, lines: s.sig === sig ? s.lines : [], note: T.logs.resync }));
        },
      },
    );
    const timer = setInterval(() => {
      if (pending.current.length === 0) return;
      const batch = pending.current;
      pending.current = [];
      setState((s) =>
        s.sig === sig
          ? { ...s, lines: appendLines(s.lines, batch) }
          : { sig, lines: appendLines([], batch), note: null },
      );
    }, 150);
    return () => {
      clearInterval(timer);
      sub.close();
    };
  }, [minLevel, prefix, epoch, sig]);

  const current = state.sig === sig ? state : null;
  return {
    lines: current?.lines ?? [],
    note: current?.note ?? null,
    clear: () => {
      pending.current = [];
      setState({ sig, lines: [], note: null });
    },
  };
}

function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

export function Logs() {
  const [level, setLevel] = useState(3);
  const [prefix, setPrefix] = useState('');
  const [text, setText] = useState('');
  const [paused, setPaused] = useState(false);
  const stream = useLogStream(level, prefix.trim());
  const [frozen, setFrozen] = useState<readonly LogEntry[] | null>(null);
  const shown = paused ? (frozen ?? stream.lines) : stream.lines;
  const visible = useMemo(() => shown.filter((e) => matchesText(e, text)), [shown, text]);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [visible]);

  return (
    <div className="adm-stack adm-logs">
      <div className="adm-row">
        <Field label={T.logs.level}>
          <Select
            value={String(level)}
            onChange={(e) => {
              setLevel(Number(e.target.value));
            }}
          >
            {LEVELS.map((l) => (
              <option key={l.value} value={l.value}>
                {l.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={T.logs.target}>
          <TextInput
            value={prefix}
            placeholder="flyff_server::admin"
            onChange={(e) => {
              setPrefix(e.target.value);
            }}
          />
        </Field>
        <Field label={T.logs.text}>
          <TextInput
            value={text}
            onChange={(e) => {
              setText(e.target.value);
            }}
          />
        </Field>
        <span className="adm-spacer" />
        <Button
          icon={paused ? 'dot' : 'clock'}
          onClick={() => {
            setFrozen(paused ? null : stream.lines);
            setPaused(!paused);
          }}
        >
          {paused ? T.logs.resume : T.logs.pause}
        </Button>
        <Button
          icon="copy"
          onClick={() => {
            if (copyText(visibleText(visible))) toast.success(T.logs.copied(visible.length));
            else toast.error(new Error('The browser did not allow copying.'));
          }}
        >
          {T.logs.copy}
        </Button>
        <Button
          icon="download"
          onClick={() => {
            download('flyff-log.txt', visibleText(visible));
          }}
        >
          {T.logs.download}
        </Button>
        <Button
          icon="x"
          variant="ghost"
          onClick={() => {
            stream.clear();
            setFrozen(null);
          }}
        >
          {T.logs.clear}
        </Button>
      </div>
      {stream.note && <p className="adm-muted">{stream.note}</p>}
      <div
        ref={box}
        className="adm-logbox"
        role="log"
        aria-live="off"
        aria-label="Server log"
        tabIndex={0}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {visible.length === 0 ? (
          <span className="adm-muted">{T.logs.waiting}</span>
        ) : (
          visible.map((e) => (
            <div
              key={e.key}
              className={`adm-logline adm-logline--${levelName(e.level).toLowerCase()}`}
            >
              {formatLogLine(e)}
            </div>
          ))
        )}
      </div>
      <p className="adm-muted">{T.logs.shown(visible.length, shown.length)}</p>
    </div>
  );
}
