// "Your access": the identity, the authority letter, the capabilities with what each grants, and how the session is connected.
import { CAP_GRANTS, isCap } from '../gen/caps';
import type { SessionInfo } from '../gen/dto';
import { Badge } from '../components/Badge';
import { Dialog } from '../components/Dialog';
import { S } from './strings';

function when(unix: number): string {
  return new Date(unix * 1000).toLocaleString();
}

export function ProfileDialog({
  info,
  onClose,
}: {
  readonly info: SessionInfo;
  readonly onClose: () => void;
}) {
  return (
    <Dialog title={S.profile.title} onClose={onClose} dismissOnOverlay>
      <dl className="adm-dl">
        <dt>{S.profile.identity}</dt>
        <dd>
          <code>{info.identity}</code>
        </dd>
        <dt>{S.profile.authority}</dt>
        <dd>
          <Badge tone="accent">{info.authority}</Badge>
        </dd>
        <dt>{S.profile.connection}</dt>
        <dd>
          {info.remote ? S.profile.remote : S.profile.local},{' '}
          {info.tls ? S.profile.tls : S.profile.plain}
        </dd>
        {info.kind === 'staff' ? (
          <>
            <dt>{S.profile.twoFactor}</dt>
            <dd data-testid="profile-totp">
              {info.totp_enabled ? S.profile.twoFactorOn : S.profile.twoFactorOff}
              {info.totp_required ? ` (${S.profile.twoFactorRequired})` : ''}
            </dd>
          </>
        ) : null}
        {info.expires_at !== null && info.idle_expires_at !== null ? (
          <>
            <dt>{S.profile.expires}</dt>
            <dd>{when(info.expires_at)}</dd>
            <dt>{S.profile.idle}</dt>
            <dd>{when(info.idle_expires_at)}</dd>
          </>
        ) : (
          <>
            <dt>{S.profile.expires}</dt>
            <dd className="adm-muted">{S.profile.none}</dd>
          </>
        )}
      </dl>
      <h3 style={{ margin: '16px 0 8px' }}>{S.profile.capabilities}</h3>
      <ul className="adm-caps">
        {info.caps.map((cap) => (
          <li key={cap}>
            <code>{cap}</code>
            <span className="adm-muted">{isCap(cap) ? CAP_GRANTS[cap] : ''}</span>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
