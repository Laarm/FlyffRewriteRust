// The vendor grid: the shared slot grid plus the name and the price beside each icon (`OnDrawItemInfo`).

import { FONT, fontStyle } from '../text/fonts';
import { cssColor } from '../text/markup';
import type { IconCellData } from '../icons/IconCell';
import { groupThousands, priceColor } from './geometry';
import type { SlotGrid } from './SlotGrid';
import { SlotGridBody } from './SlotGridView';
import { type VendorCell, vendorNameText } from './VendorGrid';

export function VendorGridView({ control }: { control: SlotGrid }) {
  return (
    <SlotGridBody
      control={control}
      extras={(slot, cell: IconCellData, o) => {
        const c = cell as VendorCell;
        return (
          <>
            <div
              data-testid="vendor-name"
              style={{
                position: 'absolute',
                left: o.x + 50,
                top: o.y + 3,
                color: '#000',
                whiteSpace: 'pre',
                pointerEvents: 'none',
                ...fontStyle(FONT.text),
              }}
            >
              {vendorNameText(c)}
            </div>
            <div
              data-testid="vendor-price"
              data-slot={slot}
              style={{
                position: 'absolute',
                left: o.x + 50,
                top: o.y + 17,
                color: cssColor(priceColor(c.price)),
                whiteSpace: 'pre',
                pointerEvents: 'none',
                ...fontStyle(FONT.text),
              }}
            >
              {groupThousands(c.price)}
            </div>
          </>
        );
      }}
    />
  );
}
