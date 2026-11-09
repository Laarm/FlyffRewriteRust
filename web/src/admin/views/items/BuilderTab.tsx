// Items & Economy -> Item builder (Plan/25 section 5.2, 5.3): the grant UI at full width. Its state lives in this module, so switching to
// another tab and back keeps the basket. `#/items/builder?item=2154` opens with that item selected.
import { useEffect } from 'react';
import type { TabProps } from '../../app/views';
import { createGrantStore, GrantPanel, GrantProvider } from '../../grant';

const tabStore = createGrantStore();

/** Tests: a fresh basket. */
export function resetBuilderTab(): void {
  tabStore.setState(createGrantStore().getState());
}

export function BuilderTab({ route }: TabProps) {
  const wanted = route.params['item'];
  useEffect(() => {
    const id = wanted === undefined ? NaN : Number(wanted);
    if (Number.isInteger(id) && id > 0) tabStore.getState().pick(id);
  }, [wanted]);
  return (
    <GrantProvider value={tabStore}>
      <GrantPanel variant="tab" />
    </GrantProvider>
  );
}
