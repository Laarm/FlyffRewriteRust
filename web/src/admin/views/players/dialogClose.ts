// Closes a dialog when the action it started succeeded (the item dialog's buttons).
export async function runConfirmedClose(done: Promise<boolean>, close: () => void): Promise<void> {
  if (await done) close();
}
