// "Save as template / kit": a name, and the item or the basket goes to `POST /admin/api/templates`.
import { useState } from 'react';
import { Button, Dialog, Field, TextInput } from '../components';
import { openDialog } from '../stores/dialogs';
import { toast } from '../stores/toasts';
import type { ItemSpec, Kit, TemplateKindDto, TemplateRowDto } from '../gen/dto';
import { createTemplate } from './api';
import { T } from './strings';

function SaveTemplateDialog({
  kind,
  item,
  kit,
  initialName,
  onSaved,
  close,
}: {
  readonly kind: TemplateKindDto;
  readonly item: ItemSpec | null;
  readonly kit: Omit<Kit, 'name'> | null;
  readonly initialName: string;
  readonly onSaved?: ((row: TemplateRowDto) => void) | undefined;
  readonly close: () => void;
}) {
  const [name, setName] = useState(initialName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async (): Promise<void> => {
    const trimmed = name.trim();
    if (trimmed === '' || busy) return;
    setBusy(true);
    setError(null);
    try {
      const row = await createTemplate({
        kind,
        name: trimmed,
        ...(kind === 'item' ? { item } : { kit: kit ? { name: trimmed, ...kit } : null }),
      });
      toast.success(T.templates.saved(trimmed));
      onSaved?.(row);
      close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };
  return (
    <Dialog
      title={kind === 'kit' ? T.templates.saveKitTitle : T.templates.saveItemTitle}
      onClose={close}
      busy={busy}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={close} disabled={busy}>
            {T.common.cancel}
          </Button>
          <Button
            variant="primary"
            loading={busy}
            disabled={name.trim() === ''}
            onClick={() => void save()}
          >
            {T.common.save}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <Field label={T.templates.name} {...(error ? { error } : {})}>
          <TextInput
            value={name}
            maxLength={64}
            data-autofocus
            onChange={(e) => {
              setName(e.target.value);
            }}
          />
        </Field>
      </form>
    </Dialog>
  );
}

/** Opens the dialog; `onSaved` runs with the stored row. */
export function openSaveTemplate(o: {
  readonly kind: TemplateKindDto;
  readonly item?: ItemSpec;
  readonly kit?: Omit<Kit, 'name'>;
  readonly name?: string;
  readonly onSaved?: (row: TemplateRowDto) => void;
}): void {
  openDialog((close) => (
    <SaveTemplateDialog
      kind={o.kind}
      item={o.item ?? null}
      kit={o.kit ?? null}
      initialName={o.name ?? ''}
      onSaved={o.onSaved}
      close={close}
    />
  ));
}
