// The toast host (Plan/25 section 2.3): the visible toasts of stores/toasts.ts, bottom right. Success and info disappear after
// 5 s, errors after 9 s; a polite live region announces them to screen readers.
import { useEffect } from 'react';
import { toast, useToasts, type Toast as ToastData } from '../stores/toasts';
import { Icon } from './Icon';

const LIFETIME_MS = { success: 5000, info: 5000, error: 9000 } as const;

function ToastView({ t }: { readonly t: ToastData }) {
  useEffect(() => {
    const id = setTimeout(() => {
      toast.dismiss(t.id);
    }, LIFETIME_MS[t.kind]);
    return () => {
      clearTimeout(id);
    };
  }, [t.id, t.kind]);
  const icon = t.kind === 'success' ? 'check' : t.kind === 'error' ? 'alert' : 'info';
  return (
    <li className={`adm-toast adm-toast--${t.kind}`} role={t.kind === 'error' ? 'alert' : 'status'}>
      <Icon name={icon} />
      <span className="adm-toast__text">
        {t.message}
        {t.code ? <code className="adm-toast__code">{t.code}</code> : null}
      </span>
      <button
        type="button"
        className="adm-iconbtn"
        aria-label="Dismiss"
        onClick={() => {
          toast.dismiss(t.id);
        }}
      >
        <Icon name="x" size={14} />
      </button>
    </li>
  );
}

export function ToastHost() {
  const toasts = useToasts((s) => s.toasts);
  return (
    <ul className="adm-toasts" aria-live="polite" aria-label="Notifications">
      {toasts.map((t) => (
        <ToastView key={t.id} t={t} />
      ))}
    </ul>
  );
}
