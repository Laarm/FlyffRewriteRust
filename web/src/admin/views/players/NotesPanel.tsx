// Moderation notes (Plan/25 section 4.3, 4.7, 4.8): the notes of a character or an account, pinned first, with a form to add one
// (`act.mute`; up to 2,000 characters; a pinned note shows as a banner in the subject's drawer) and a delete.
import { useState } from 'react';
import { runConfirmed } from '../../app/mutate';
import { Button, Checkbox, Field, TextArea, Badge } from '../../components';
import type { NoteView } from '../../gen/dto';
import { useCan } from '../../stores/session';
import { toast } from '../../stores/toasts';
import { addNote, deleteNote } from './api';
import { formatDateTime } from './format';
import { T } from './strings';

export function NoteList({
  notes,
  onChanged,
  showSubject = false,
}: {
  readonly notes: readonly NoteView[];
  readonly onChanged: () => void;
  readonly showSubject?: boolean;
}) {
  const canEdit = useCan('act.mute');
  if (notes.length === 0) return <p className="adm-muted">{T.notes.none}</p>;
  return (
    <ul className="plr-notes">
      {notes.map((n) => (
        <li key={n.id} className="plr-note" data-testid="note">
          <div className="plr-note__head">
            {n.pinned ? <Badge tone="warn">{T.notes.pinned}</Badge> : null}
            {showSubject ? (
              <Badge>
                {n.subject_kind === 'account' ? T.notes.forAccount : T.notes.forCharacter} #
                {n.subject_id}
              </Badge>
            ) : null}
            <span className="adm-muted adm-small">
              {T.notes.by(n.author, formatDateTime(n.created_at))}
            </span>
            {canEdit ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  void runConfirmed({
                    title: T.notes.deleteTitle,
                    confirmLabel: T.notes.delete,
                    danger: true,
                    message: <blockquote className="plr-quote">{n.text}</blockquote>,
                    run: () => deleteNote(n.id),
                    success: T.notes.deleted,
                  }).then((o) => {
                    if (o.ok) onChanged();
                  });
                }}
              >
                {T.notes.delete}
              </Button>
            ) : null}
          </div>
          <p className="plr-note__text">{n.text}</p>
        </li>
      ))}
    </ul>
  );
}

export function AddNote({
  kind,
  subjectId,
  onAdded,
}: {
  readonly kind: 'account' | 'character';
  readonly subjectId: number;
  readonly onAdded: () => void;
}) {
  const canEdit = useCan('act.mute');
  const [text, setText] = useState('');
  const [pinned, setPinned] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!canEdit) return <p className="adm-muted adm-small">{T.notes.needsMute}</p>;
  const submit = async (): Promise<void> => {
    if (text.trim() === '') return;
    setBusy(true);
    try {
      await addNote({ subject_kind: kind, subject_id: subjectId, text: text.trim(), pinned }, {});
      toast.success(T.notes.added);
      setText('');
      setPinned(false);
      onAdded();
    } catch (error) {
      toast.error(error, T.notes.add);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="adm-stack"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <Field label={T.notes.text}>
        <TextArea
          rows={3}
          maxLength={2000}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
        />
      </Field>
      <Checkbox
        label={T.notes.pin}
        checked={pinned}
        onChange={(e) => {
          setPinned(e.target.checked);
        }}
      />
      <div>
        <Button type="submit" variant="primary" loading={busy} disabled={text.trim() === ''}>
          {T.notes.add}
        </Button>
      </div>
    </form>
  );
}
