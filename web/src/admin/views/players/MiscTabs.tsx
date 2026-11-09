// The smaller tabs of the character drawer: Skills, Quests, Social, History and Notes (Plan/25 section 4.3).
import { useMemo, useState } from 'react';
import { RowLookup } from '../../../data/lookup';
import { Badge, Button, Field, Select, Table, type Column } from '../../components';
import { navigate } from '../../stores/route';
import type { QuestEntry, SkillEntry } from '../../gen/dto';
import { AuditTimeline } from './AuditTimeline';
import { useCharacter } from './context';
import { useBundle } from './gamedata';
import { AddNote, NoteList } from './NotesPanel';
import { T } from './strings';
import { GuildChip } from '../guilds/GuildChip';

export function SkillsTab() {
  const { live } = useCharacter();
  const skills = useBundle('skills');
  const lookup = useMemo(
    () => (skills.data ? new RowLookup(skills.data.skills) : null),
    [skills.data],
  );
  const d = live.detail;
  if (!d) return null;
  const rows = d.skills.filter((s) => s.level > 0);
  const nameOf = (s: SkillEntry): string =>
    lookup?.get(s.skill)?.name ?? `Skill ${String(s.skill)}`;
  const columns: Column<SkillEntry>[] = [
    {
      id: 'skill',
      header: T.skills.skill,
      cell: nameOf,
      text: nameOf,
      sortable: true,
      filterable: true,
    },
    {
      id: 'id',
      header: 'Id',
      cell: (s) => s.skill,
      text: (s) => s.skill,
      sortable: true,
      width: '70px',
    },
    {
      id: 'level',
      header: T.skills.level,
      cell: (s) => s.level,
      text: (s) => s.level,
      sortable: true,
      align: 'right',
      width: '80px',
    },
  ];
  return (
    <div className="plr-section" data-testid="skills-tab">
      <p>
        <strong>{T.skills.free}:</strong> {d.skill_points}
      </p>
      {skills.data === null && skills.error === null ? (
        <p className="adm-muted">{T.skills.waiting}</p>
      ) : null}
      <Table
        label="Skills"
        columns={columns}
        rows={rows}
        rowKey={(s) => String(s.skill)}
        emptyTitle={T.skills.none}
        emptyHint={T.skills.legend}
      />
    </div>
  );
}

export function QuestsTab() {
  const { live } = useCharacter();
  const d = live.detail;
  if (!d) return null;
  const active = d.quests.filter((q) => q.state < 14);
  const finished = d.quests.filter((q) => q.state >= 14);
  const columns = (withState: boolean): Column<QuestEntry>[] => [
    {
      id: 'quest',
      header: T.quests.id,
      cell: (q) => `#${String(q.quest)}`,
      text: (q) => q.quest,
      sortable: true,
      filterable: true,
    },
    ...(withState
      ? [
          {
            id: 'state',
            header: 'Progress',
            cell: (q: QuestEntry) => T.quests.state(q.state),
            text: (q: QuestEntry) => q.state,
          } as Column<QuestEntry>,
        ]
      : []),
  ];
  return (
    <div className="plr-section" data-testid="quests-tab">
      <section className="plr-section">
        <h3>
          {T.quests.active} <Badge>{active.length}</Badge>
        </h3>
        <Table
          label={T.quests.active}
          columns={columns(true)}
          rows={active}
          rowKey={(q) => String(q.quest)}
          emptyTitle={T.quests.none}
        />
      </section>
      <section className="plr-section">
        <h3>
          {T.quests.finished} <Badge>{finished.length}</Badge>
        </h3>
        <Table
          label={T.quests.finished}
          columns={columns(false)}
          rows={finished}
          rowKey={(q) => String(q.quest)}
          emptyTitle={T.quests.none}
        />
      </section>
      <p className="adm-muted adm-small">{T.quests.phaseC}</p>
    </div>
  );
}

export function SocialTab() {
  const { live } = useCharacter();
  const d = live.detail;
  if (!d) return null;
  return (
    <div className="plr-section" data-testid="social-tab">
      <dl className="adm-dl">
        <dt>{T.social.guild}</dt>
        <dd>
          {d.guild_id === 0 ? (
            T.social.none
          ) : (
            <Button
              size="sm"
              onClick={() => {
                navigate({ category: 'guilds', tab: null, params: { g: String(d.guild_id) } });
              }}
            >
              {T.social.openGuild} <GuildChip id={d.guild_id} />
            </Button>
          )}
        </dd>
        <dt>{T.social.party}</dt>
        <dd>{d.party_id === 0 ? T.social.none : `#${String(d.party_id)}`}</dd>
      </dl>
      <p className="adm-muted adm-small">{T.social.guildHint}</p>
      <p className="adm-muted adm-small">{T.social.needsLater}</p>
    </div>
  );
}

export function HistoryTab() {
  const { id } = useCharacter();
  const subject = useMemo(() => ({ character: id }), [id]);
  return <AuditTimeline subject={subject} empty={T.history.empty} />;
}

export function NotesTab() {
  const { id, live } = useCharacter();
  const [about, setAbout] = useState<'character' | 'account'>('character');
  const profile = live.profile;
  if (!profile) return null;
  return (
    <div className="plr-section" data-testid="notes-tab">
      <h3>{T.notes.title}</h3>
      <NoteList notes={profile.notes} onChanged={live.reload} showSubject />
      <h3>{T.notes.add}</h3>
      <Field label={T.moderation.notes.subject}>
        <Select
          value={about}
          onChange={(e) => {
            setAbout(e.target.value === 'account' ? 'account' : 'character');
          }}
        >
          <option value="character">{T.notes.forCharacter}</option>
          <option value="account">{T.notes.forAccount}</option>
        </Select>
      </Field>
      <AddNote
        kind={about}
        subjectId={about === 'character' ? id : profile.character.account_id}
        onAdded={live.reload}
      />
    </div>
  );
}
