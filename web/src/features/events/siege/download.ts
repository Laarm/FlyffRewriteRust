// The browser download of the kill log (Plan/45 §28.4; `CWndGuildCombatTabResultLog::OnChildNotify`, `GuildLog_YYYYMMDD.txt`): the original wrote the file next to the executable; the browser
// saves it through a link. It must run inside the click handler of the save button (a download needs the user's gesture).

/** Saves `text` as a UTF-8 file named `name`; `false` when the browser cannot do it. */
export function saveTextFile(name: string, text: string): boolean {
  if (typeof document === 'undefined' || typeof URL.createObjectURL !== 'function') return false;
  try {
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.style.display = 'none';
    document.body.append(a);
    a.click();
    a.remove();
    // the click has started the download; the URL may go a moment later
    setTimeout(() => {
      URL.revokeObjectURL(url);
    }, 0);
    return true;
  } catch {
    return false;
  }
}
