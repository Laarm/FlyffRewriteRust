// Logger with a ring buffer (Plan/40 section 12.5, 02 section 17). The ONE place that may call console;
// the last `capacity` entries are shown on the fatal screen and exposed to the debug probe.

export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

export interface LogEntry {
  /** `performance.now()` at the call (ms). */
  readonly at: number;
  readonly level: LogLevel;
  readonly scope: string;
  readonly message: string;
}

export interface Logger {
  debug(scope: string, message: string): void;
  info(scope: string, message: string): void;
  warn(scope: string, message: string): void;
  error(scope: string, message: string): void;
  /** The newest `n` entries (a copy), oldest first. */
  tail(n?: number): readonly LogEntry[];
  /** Formatted lines of {@link Logger.tail}. */
  lines(n?: number): string[];
}

export interface LoggerOptions {
  /** Ring buffer size (default 500). */
  capacity?: number;
  /** Mirror entries to the browser console (default true). */
  console?: boolean;
  /** Time source (default `performance.now`). */
  now?: () => number;
}

const LEVEL_ORDER: Record<LogLevel, number> = { debug: 0, info: 1, warn: 2, error: 3 };

export function formatEntry(e: LogEntry): string {
  return `${e.at.toFixed(0).padStart(8)} ${e.level.toUpperCase().padEnd(5)} [${e.scope}] ${e.message}`;
}

export function createLogger(options: LoggerOptions = {}): Logger {
  const capacity = Math.max(1, options.capacity ?? 500);
  const mirror = options.console ?? true;
  const now = options.now ?? (() => performance.now());
  const ring: LogEntry[] = [];

  function write(level: LogLevel, scope: string, message: string): void {
    ring.push({ at: now(), level, scope, message });
    if (ring.length > capacity) {
      ring.splice(0, ring.length - capacity);
    }
    if (mirror && LEVEL_ORDER[level] >= LEVEL_ORDER.info) {
      const line = `[${scope}] ${message}`;
      if (level === 'error') console.error(line);
      else if (level === 'warn') console.warn(line);
      else console.info(line);
    }
  }

  const tail = (n = capacity): readonly LogEntry[] => ring.slice(Math.max(0, ring.length - n));
  return {
    debug: (scope, message) => {
      write('debug', scope, message);
    },
    info: (scope, message) => {
      write('info', scope, message);
    },
    warn: (scope, message) => {
      write('warn', scope, message);
    },
    error: (scope, message) => {
      write('error', scope, message);
    },
    tail,
    lines: (n) => tail(n).map(formatEntry),
  };
}

/** The page-wide logger (platform singletons are the one allowed kind of module state, Plan/40 CA3). */
export const log: Logger = createLogger();
