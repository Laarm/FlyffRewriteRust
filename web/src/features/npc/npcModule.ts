// The `npc` feature module (Plan/45 §11-§12, §17; WP-CFEAT-18, -19): one module that registers its parts in order - the menu
// table first (the others add their actions to it), then the dialog, the shop and the beauty shops.

import { type FeatureModule } from '../common/module';
import { beautyOf, registerBeauty } from './beauty/beautyModule';
import { registerNpcBuff } from './buffMenu';
import { dialogOf, registerDialog } from './dialog/dialogModule';
import { menuOf, registerMenu } from './menuModule';
import { registerShop, shopOf } from './shop/shopModule';

export const npcModule: FeatureModule = {
  id: 'npc',
  milestone: 4,
  register(fc) {
    const menu = registerMenu(fc);
    registerDialog(fc, menu);
    registerShop(fc, menu);
    registerBeauty(fc, menu);
    registerNpcBuff(fc, menu);
  },
};

export { beautyOf, dialogOf, menuOf, shopOf };
