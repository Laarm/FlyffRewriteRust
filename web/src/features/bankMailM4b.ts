// The feature modules of the ui-bank-mail task (WP-CFEAT-20, -23, and the pockets part of -29) as handler modules of `app/features.ts` (`FEATURE_MODULES`): the bank with its PIN flow,
// the mail windows and the backpack (pockets). Registered after the NPC modules (the menu table the actions go into) and the cfeat-m3 modules (the inventory they drop into).

import type { HandlerModule } from '../app/features';
import { bankModule } from './bank';
import { toHandlerModule } from './common/module';
import { mailModule } from './mail';
import { pocketsModule } from './pockets';

export const BANK_MAIL_M4B_FEATURE_MODULES: readonly HandlerModule[] = [
  bankModule,
  mailModule,
  pocketsModule,
].map(toHandlerModule);
