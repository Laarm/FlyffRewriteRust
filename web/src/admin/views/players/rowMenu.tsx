// The row menu of a character in a table (Plan/25 section 4.1): Profile, Show on map, Inventory, Grant item, Message, Kick, Mute, Ban.
// Only the actions the session may do are listed; the ones that need the character online are disabled for an offline one.
import { withParams, withRecord } from '../../app/route';
import { Menu, type MenuItem } from '../../components';
import { getRoute, navigate } from '../../stores/route';
import { useCaps } from '../../stores/session';
import { ban, grantTo, kick, message, mute, showOnMap, type Target } from './characterActions';
import { T } from './strings';

/** Opens the character in the drawer (`tab`: the tab of the drawer, `items` for the inventory). */
export function openCharacter(id: number, tab?: string): void {
  const route = getRoute() ?? { category: 'players', tab: 'online', params: {} };
  const next = withRecord(route, { kind: 'character', id: String(id) });
  navigate(tab ? withParams(next, { ct: tab }) : withParams(next, { ct: undefined }));
}

export function menuItems(t: Target, caps: ReadonlySet<string>): MenuItem[] {
  const online = t.online === true;
  const gate = (ok: boolean): { disabled: boolean; hint?: string } => ({
    disabled: !ok,
    ...(ok ? {} : { hint: T.actions.onlineOnly }),
  });
  const items: MenuItem[] = [
    {
      id: 'profile',
      label: T.actions.profile,
      onSelect: () => {
        openCharacter(t.id);
      },
    },
  ];
  if (caps.has('view.inventory')) {
    items.push({
      id: 'inventory',
      label: T.actions.inventory,
      onSelect: () => {
        openCharacter(t.id, 'items');
      },
    });
  }
  if (caps.has('view.map')) {
    items.push({
      id: 'map',
      label: T.actions.showOnMap,
      ...gate(online),
      onSelect: () => {
        showOnMap(t);
      },
    });
  }
  if (caps.has('act.items')) {
    items.push({
      id: 'grant',
      label: T.actions.grant,
      onSelect: () => {
        grantTo(t);
      },
    });
  }
  if (caps.has('act.message.player')) {
    items.push({
      id: 'message',
      label: T.actions.message,
      separator: true,
      ...gate(online),
      onSelect: () => void message([t]),
    });
  }
  if (caps.has('act.kick')) {
    items.push({
      id: 'kick',
      label: T.actions.kick,
      ...gate(online),
      onSelect: () => void kick(t),
    });
  }
  if (caps.has('act.mute')) {
    items.push({
      id: 'mute',
      label: T.actions.mute,
      ...gate(online),
      onSelect: () => void mute(t),
    });
  }
  if (caps.has('act.ban')) {
    items.push({ id: 'ban', label: T.actions.ban, onSelect: () => void ban(t) });
  }
  return items;
}

/** The "..." button of a row. */
export function RowMenu({ target }: { readonly target: Target }) {
  const caps = useCaps();
  return (
    <span
      onClick={(e) => {
        e.stopPropagation();
      }}
      onKeyDown={(e) => {
        e.stopPropagation();
      }}
    >
      <Menu
        label={`Actions for ${target.name}`}
        trigger={<span aria-hidden="true">...</span>}
        items={menuItems(target, caps)}
      />
    </span>
  );
}
