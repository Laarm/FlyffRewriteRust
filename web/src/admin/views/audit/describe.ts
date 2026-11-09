// One readable line for an audit record ("Mute character 5 for 1h 0m"), for the "My actions" list. A record of an event this does not
// know keeps the server's summary. Pure; tested.
import { formatDuration } from '../dashboard/format';
import { isRecord, num, numbers, str } from './api';

function who(d: Readonly<Record<string, unknown>>): string {
  const c = num(d['character'] ?? d['target']);
  return c === null ? 'a character' : `character ${String(c)}`;
}

function signed(n: number): string {
  return `${n > 0 ? '+' : ''}${n.toLocaleString('en-US')}`;
}

/** `summary` without the leading state word the server puts first (`done, reason: Spam, ...`). */
export function withoutState(summary: string): string {
  return summary.replace(/^(requested|done|failed)(, )?/, '');
}

export function describeAction(
  ev: string,
  d: Readonly<Record<string, unknown>>,
  summary: string,
): string {
  switch (ev) {
    case 'admin.mute': {
      if (str(d['action']) === 'unmute') return `Unmute ${who(d)}`;
      const seconds = num(d['duration']);
      return `Mute ${who(d)} ${seconds === null || d['permanent'] === true ? 'permanently' : `for ${formatDuration(seconds)}`}`;
    }
    case 'admin.teleport': {
      const to = d['to'];
      const world = isRecord(to) ? num(to['world']) : null;
      const mode = str(d['mode']);
      return mode === 'unstuck'
        ? `Return ${who(d)} to town`
        : `Teleport ${who(d)}${world === null ? '' : ` to world ${String(world)}`}`;
    }
    case 'admin.character_edit': {
      const field = str(d['field']);
      if (field === 'login_block')
        return `${d['lift'] === true ? 'Lift the login block of' : 'Block the login of'} ${who(d)}`;
      if (field === 'mode_flags') return `Change the mode flags of ${who(d)}`;
      if (field === null && str(d['account']) !== null)
        return `Ban account ${str(d['account']) ?? ''}`;
      return `Edit ${who(d)}${field === null ? '' : ` (${field})`}`;
    }
    case 'admin.ip_block':
      return `${str(d['action']) === 'remove' ? 'Unblock' : 'Block'} ${str(d['cidr']) ?? 'a network'}`;
    case 'admin.grant': {
      const n = num(d['recipients']) ?? 0;
      return `Grant to ${String(n)} ${n === 1 ? 'recipient' : 'recipients'} (${str(d['mode']) ?? 'auto'})`;
    }
    case 'admin.grant_cancel':
      return `Cancel pending grant #${String(num(d['id']) ?? 0)}`;
    case 'admin.removal_cancel':
      return `Cancel pending removal #${String(num(d['id']) ?? 0)}`;
    case 'admin.removal': {
      const mode = str(d['mode']);
      return `Remove ${String(num(d['count']) ?? 1)} of item ${String(num(d['item_id']) ?? '?')} from ${who(d)}${mode === null ? '' : ` (${mode})`}`;
    }
    case 'admin.gold': {
      const delta = num(d['delta']);
      return `Adjust the penya of ${who(d)}${delta === null ? '' : ` by ${signed(delta)}`}`;
    }
    case 'admin.spawn': {
      const n = num(d['count']);
      const spawned = numbers(d['spawned']).length;
      return `Spawn ${String(n ?? spawned)} monsters${num(d['world']) === null ? '' : ` in world ${String(num(d['world']))}`}`;
    }
    case 'admin.kick':
      return `Kick ${str(d['account']) ?? who(d)}`;
    case 'admin.guild':
      return `Guild ${str(d['action']) ?? 'change'}${num(d['guild']) === null ? '' : ` (guild ${String(num(d['guild']))})`}`;
    case 'admin.config':
      return `Set ${str(d['key']) ?? 'a configuration key'}`;
    case 'admin.job':
      return `Run job ${str(d['job']) ?? str(d['name']) ?? ''}`.trim();
    case 'admin.backup':
      return `Backup: ${str(d['action']) ?? 'run'}`;
    case 'account.ban':
      return `Ban account ${str(d['account']) ?? ''}`.trim();
    default:
      return withoutState(summary) || ev;
  }
}
