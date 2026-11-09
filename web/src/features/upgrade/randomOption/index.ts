// The random-option retry scroll (`APP_RANDOMOPTIONRETRY`, `features.randomoption_retry`, Plan/45 §18.5; WP-CFEAT-25): a target slot (an awakened item with more than one line),
// the scroll reserved while the window is open, one radio per line that exists -> `RandomOptionRetry{item, scroll, line}`. The window is `ui.itemUse.openRandomOptionRetry`
// (WP-UI-29, `ui/windows/itemuse/WndRandomOptionRetry.ts`) and the double click of the scroll opens it through the item dispatch (rule D6); both are wired by the
// `itemuse` feature (`features/itemuse`, `randomOptionRetry`), which owns the send. This folder pins the contract of 45 §18.5 end to end in
// `randomOption.flow.test.tsx` next to the other upgrade windows, and holds nothing else: there is no second implementation to keep in step.
export { itemUseOf } from '../../itemuse';
