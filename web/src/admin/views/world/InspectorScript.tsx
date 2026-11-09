// "Open NPC script" (Plan/25 section 3.10): the transpiled dialog of an NPC (`scripts/npc/<key>.lua` of the server's home) in a read-only window.
// `GET /admin/api/npc-scripts/{key}`; the text is cut at 256 KiB by the server and the window says so.
import { useEffect, useState } from 'react';
import { copyText } from '../../app/clipboard';
import { rest } from '../../api/rest';
import { Button, Dialog, ErrorState, Skeleton } from '../../components';
import type { NpcScript } from '../../gen/dto';
import { openDialog } from '../../stores/dialogs';
import { toast } from '../../stores/toasts';
import { I } from './InspectorStrings';

function ScriptWindow({
  keyName,
  onClose,
}: {
  readonly keyName: string;
  readonly onClose: () => void;
}) {
  const [state, setState] = useState<
    { readonly script: NpcScript } | { readonly error: unknown } | null
  >(null);
  useEffect(() => {
    let live = true;
    rest.get<NpcScript>(`/admin/api/npc-scripts/${encodeURIComponent(keyName)}`).then(
      (script) => {
        if (live) setState({ script });
      },
      (error: unknown) => {
        if (live) setState({ error });
      },
    );
    return () => {
      live = false;
    };
  }, [keyName]);
  const script = state && 'script' in state ? state.script : null;
  return (
    <Dialog
      title={script?.file ?? `${I.script.title}: ${keyName}`}
      onClose={onClose}
      size="lg"
      footer={
        <>
          {script ? (
            <Button
              variant="ghost"
              onClick={() => {
                if (copyText(script.text)) toast.success(I.script.copied);
                else toast.error(new Error(I.copyFailed));
              }}
            >
              {I.script.copy}
            </Button>
          ) : null}
          <Button variant="primary" onClick={onClose}>
            {I.script.close}
          </Button>
        </>
      }
    >
      {state === null ? <Skeleton lines={8} /> : null}
      {state && 'error' in state ? (
        <ErrorState error={state.error} title={I.script.failed} />
      ) : null}
      {script ? (
        <>
          <pre className="insp-script" data-testid="npc-script" tabIndex={0}>
            {script.text}
          </pre>
          {script.truncated ? (
            <p className="insp-note insp-note--warn">{I.script.truncated}</p>
          ) : null}
        </>
      ) : null}
    </Dialog>
  );
}

/** Opens the read-only window of a script by its key (the `dialog_key` of the `npcs` bundle). */
export function openNpcScript(key: string): void {
  openDialog((close) => <ScriptWindow keyName={key} onClose={close} />);
}
