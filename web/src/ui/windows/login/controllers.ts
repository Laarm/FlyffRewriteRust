// The login-state shells of the window catalog (`BUILT_CONTROLLERS` of `catalog.ts`, WP-UI-11): `APP_*` name -> lazy loader.
import type { WndCtor } from '../../catalog';

export const LOGIN_CONTROLLERS: Readonly<Record<string, () => Promise<WndCtor>>> = {
  APP_LOGIN: async () => (await import('./WndLogin')).WndLogin,
  APP_SELECT_CHAR: async () => (await import('./WndSelectChar')).WndSelectChar,
  APP_CREATE_CHAR: async () => (await import('./WndCreateChar')).WndCreateChar,
  APP_DELETE_CHAR: async () => (await import('./WndDeleteChar')).WndDeleteChar,
  APP_CLOSE_EXISTING_CONNECTION: async () =>
    (await import('./WndCloseExisting')).WndCloseExistingConnection,
};
