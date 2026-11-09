// A guild as a small logo and its name (Plan/25 section 6.1: the guild index replaces `Guild #id` in the player views). The name and the logo
// come from `useGuildIndex()` (one cached list read); until it loads, and for a viewer who may not list guilds, the chip is the plain `#id`.
import { GuildLogo } from '../../components/GuildLogo';
import { useGuildIndex } from './useGuildMeta';

export function GuildChip({
  id,
  scale = 1,
  prefix = '',
}: {
  readonly id: number;
  readonly scale?: number;
  /** Text before the chip ("Guild "), kept in one text node with the `#id` fallback. */
  readonly prefix?: string;
}) {
  const index = useGuildIndex();
  const g = index.get(id);
  if (!g) return <span data-guild={id}>{`${prefix}#${String(id)}`}</span>;
  return (
    <span className="adm-row" data-guild={id}>
      {prefix === '' ? null : <span>{prefix}</span>}
      <GuildLogo logo={g.logo} scale={scale} label={`${g.name} logo`} />
      <span>{g.name}</span>
    </span>
  );
}
