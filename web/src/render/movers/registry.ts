// The model/item/mover registry the mover visuals read (Plan/41 section 12.2-12.3, WP-REND-05; Plan/14 section 10.4
// `models`, `items`, `movers` bundles). It extracts ONLY the columns the assembly rules need from the bundle row
// tables (the `items` bundle has 109 columns and 15 000 rows) and answers the registry questions of
// `CModelMng` (ref/18 section 10):
//
//   MakeModelName(OT_ITEM, id)   the `file` of the `models.types["4"]` row (`item_<name>.o3d`)
//   MakePartsName("part", id, sex)  `part_<m_szPart split by "/" for male / female>.o3d`
//   GetModelElem fallback        a missing item entry shows `II_ARM_M_VAG_HELMET02` (ModelMng.cpp:38-54)
//   MoverProp.bIfParts           humanoid movers (players, `NpcPart`) are assembled from parts

import { II_ARM_M_VAG_HELMET02 } from '../../gen/defs/item';
import { OT_CTRL, OT_ITEM, OT_MOVER } from '../../gen/defs/define';
import type {
  ItemsBundle,
  ModelRow,
  ModelsBundle,
  MoversBundle,
  RowTable,
} from '../../gen/bundles';

/** `NULL_ID` as a JSON number (`u32::MAX`) or as the signed value some columns carry. */
export const NULL_ID = 0xffff_ffff;

/** `true` for `NULL_ID` (`0xFFFFFFFF` / `-1`, the original's `(DWORD)-1`). */
export const isNullId = (v: number | null | undefined): boolean =>
  v === undefined || v === null || v === NULL_ID || v === -1;

/** The item properties the part rules read (`ItemProp`). */
export interface ItemVisualProp {
  readonly id: number;
  /** `dwParts`: the `PARTS_*` slot of the item. */
  readonly parts: number;
  /** `bPartsFile`: `1` means the model is a `part_*.o3d` split by sex. */
  readonly partsFile: number;
  /** `dwExclusive`: the part that is taken off when this item is worn (`NULL_ID` = none). */
  readonly exclusive: number;
  /** `dwBasePartsIgnore`: the base part this cap/hat hides (`NULL_ID` = none). */
  readonly basePartsIgnore: number;
  /** `dwItemKind3` (`IK3_*`). */
  readonly kind3: number;
  /** `nReflect` (a shield with a value > 0 is drawn with the reflect map). */
  readonly reflect: number;
  /** `dwHanded` (`HD_*`). */
  readonly handed: number;
}

/** The mover properties the assembly reads. */
export interface MoverVisualProp {
  readonly id: number;
  /** `bIfParts`. */
  readonly ifParts: boolean;
  /** `dwClass` (`RANK_*`, `RANK_SUPER` bosses do not shake). */
  readonly rank: number;
  /** `dwAI` (`AII_PET`, `AII_EGG` are hidden by the "hide pets" option). */
  readonly ai: number;
}

type Cell = unknown;

function columnIndex(t: RowTable<unknown>, names: readonly string[]): number[] {
  return names.map((n) => t.columns.indexOf(n));
}

const num = (row: readonly Cell[], at: number, fallback = 0): number => {
  const v = at >= 0 ? row[at] : undefined;
  return typeof v === 'number' ? v : fallback;
};

/** Builds the item index of an `items` bundle (only the columns of {@link ItemVisualProp}). */
export function indexItems(bundle: ItemsBundle): Map<number, ItemVisualProp> {
  const t = bundle.items;
  const [iId, iParts, iFile, iExcl, iBpi, iKind3, iReflect, iHanded] = columnIndex(t, [
    'id',
    'parts',
    'parts_file',
    'exclusive',
    'base_parts_ignore',
    'item_kind3',
    'reflect',
    'handed',
  ]) as [number, number, number, number, number, number, number, number];
  const out = new Map<number, ItemVisualProp>();
  for (const row of t.rows) {
    const id = num(row, iId, -1);
    if (id < 0 || out.has(id)) continue;
    out.set(id, {
      id,
      parts: num(row, iParts, NULL_ID),
      partsFile: num(row, iFile, 0),
      exclusive: num(row, iExcl, NULL_ID),
      basePartsIgnore: num(row, iBpi, NULL_ID),
      kind3: num(row, iKind3, NULL_ID),
      reflect: num(row, iReflect, 0),
      handed: num(row, iHanded, 0),
    });
  }
  return out;
}

/** Builds the mover index of a `movers` bundle. */
export function indexMovers(bundle: MoversBundle): Map<number, MoverVisualProp> {
  const t = bundle.movers;
  const [iId, iIf, iClass, iAi] = columnIndex(t, ['id', 'if_parts', 'class', 'ai']) as [
    number,
    number,
    number,
    number,
  ];
  const out = new Map<number, MoverVisualProp>();
  for (const row of t.rows) {
    const id = num(row, iId, -1);
    if (id < 0 || out.has(id)) continue;
    out.set(id, {
      id,
      ifParts: num(row, iIf, 0) === 1,
      rank: num(row, iClass, 0),
      ai: num(row, iAi, 0),
    });
  }
  return out;
}

/** Builds `index -> ModelRow` of one object type of the `models` bundle. */
export function indexModels(bundle: ModelsBundle, ot: number): Map<number, ModelRow> {
  const t = bundle.types[String(ot)];
  const out = new Map<number, ModelRow>();
  if (!t) return out;
  const cols = t.columns;
  for (const row of t.rows) {
    const r: Record<string, unknown> = {};
    cols.forEach((c, i) => {
      r[c] = row[i];
    });
    const m = r as unknown as ModelRow;
    if (typeof m.index === 'number' && !out.has(m.index)) out.set(m.index, m);
  }
  return out;
}

/** `MakePartsName(root = "part")`: `model/part_<male|female part>.o3d`, lower-case. */
export function partsFileName(part: string, sex: 0 | 1): string {
  const slash = part.indexOf('/');
  const name = slash < 0 ? part : sex === 0 ? part.slice(0, slash) : part.slice(slash + 1);
  return `model/part_${name}.o3d`.toLowerCase();
}

/** The default body parts of a sex (`PARTSMESH_UPPER/LOWER/HAND/FOOT`). */
export function defaultBodyFile(kind: 'upper' | 'lower' | 'hand' | 'foot', sex: 0 | 1): string {
  return `model/part_${sex === 0 ? 'male' : 'female'}${kind}.o3d`;
}

/** `PARTSMESH_HAIR`/`PARTSMESH_HEAD` of a mesh index (`hairMesh + 1`, two digits). */
export function hairFileName(hairMesh: number, sex: 0 | 1): string {
  return `model/part_${sex === 0 ? 'male' : 'female'}hair${String(hairMesh + 1).padStart(2, '0')}.o3d`;
}

export function headFileName(headMesh: number, sex: 0 | 1): string {
  return `model/part_${sex === 0 ? 'male' : 'female'}head${String(headMesh + 1).padStart(2, '0')}.o3d`;
}

/** The registry of the visual rules. */
export class VisualRegistry {
  private readonly itemProps: Map<number, ItemVisualProp>;
  private readonly moverProps: Map<number, MoverVisualProp>;
  private readonly itemModels: Map<number, ModelRow>;
  private readonly moverModels: Map<number, ModelRow>;
  private readonly ctrlModels: Map<number, ModelRow>;

  constructor(
    models: ModelsBundle,
    items: ItemsBundle | Map<number, ItemVisualProp>,
    movers: MoversBundle | Map<number, MoverVisualProp>,
  ) {
    this.itemProps = items instanceof Map ? items : indexItems(items);
    this.moverProps = movers instanceof Map ? movers : indexMovers(movers);
    this.itemModels = indexModels(models, OT_ITEM);
    this.moverModels = indexModels(models, OT_MOVER);
    this.ctrlModels = indexModels(models, OT_CTRL);
  }

  item(id: number): ItemVisualProp | undefined {
    return this.itemProps.get(id);
  }

  mover(id: number): MoverVisualProp | undefined {
    return this.moverProps.get(id);
  }

  /** The registry entry of an item model; a missing entry shows the helmet of `GetModelElem`'s fallback. */
  itemModel(id: number): ModelRow | undefined {
    return this.itemModels.get(id) ?? this.itemModels.get(II_ARM_M_VAG_HELMET02);
  }

  /** The registry entry of a mover model (`MI_*`). */
  moverModel(index: number): ModelRow | undefined {
    return this.moverModels.get(index);
  }

  /** The registry entry of a ctrl model (`CI_*`, `models.types["2"]`). */
  ctrlModel(index: number): ModelRow | undefined {
    return this.ctrlModels.get(index);
  }

  /**
   * The `.o3d` an equipped item loads (`UpdateParts`): `part_*.o3d` for `bPartsFile == 1`, else the registry file
   * (`MakeModelName`). `null` when the registry has no file (the part then shows nothing, KQ-30).
   */
  itemFile(id: number, sex: 0 | 1): string | null {
    const prop = this.itemProps.get(id);
    const e = this.itemModel(id);
    if (!prop || !e) return null;
    if (prop.partsFile === 1) return partsFileName(e.part, sex);
    return e.file;
  }

  /** Number of items / movers indexed (probes). */
  get counts(): {
    items: number;
    movers: number;
    itemModels: number;
    moverModels: number;
    ctrlModels: number;
  } {
    return {
      items: this.itemProps.size,
      movers: this.moverProps.size,
      itemModels: this.itemModels.size,
      moverModels: this.moverModels.size,
      ctrlModels: this.ctrlModels.size,
    };
  }
}
