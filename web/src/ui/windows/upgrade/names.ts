// The pictures and labels of the upgrade windows, apart from the shells so that the service and the features can read them without loading them
// (`ui/wnd/lazy.ts`).

/** The theme pictures of `CWndNewUpgrade::OnInitialUpdate` (`SafetyGauge.bmp` is three 4 px slices, the result pictures are 168 x 32). */
export const SAFETY_GAUGE = 'SafetyGauge.bmp';
export const SAFETY_SUCCESS = 'SafetySuccess.bmp';
export const SAFETY_FAILURE = 'SafetyFailure.bmp';

/** The labels of the two buttons of the protected-awakening choice (the original has pictures there and no text, see `WndSelectAwakeCase.ts`). */
export const KEEP_LABEL = 'Keep';
export const TAKE_LABEL = 'Select';
