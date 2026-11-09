// 3D preview presets (Plan/41 section 21.2, WP-REND-06; WndTitle.cpp:1700-1880, ref/21 section 4). The camera and the
// model transform of the character-select / -create slots, the lighting of every preview ("lighting off": the
// diffuse and ambient light are white, the skinned colour saturates to 1, no fog, the hair ambient is the hair
// colour) and the motion sequences the window asks the simulation for.

import {
  MTI_GETUP,
  MTI_RUN,
  MTI_SIT,
  MTI_SITSTAND,
  MTI_STAND,
  MTI_STAND2,
} from '../../gen/defs/neuz';
import type { EnvLight } from '../gl/materials/env';
import { mirrorMatrix } from '../space';
import { multiply, rotationY, scaling, transformCoord, translation } from '../d3dx';
import type { CameraState, Vec3LH } from '../types';

export type PreviewPresetName =
  | 'charSelect'
  | 'charCreate'
  | 'inventory'
  | 'statusPortrait'
  | 'modelView'
  | 'beautyHair'
  | 'hairKind'
  | 'faceKind'
  | 'changeSex'
  | 'petStatus'
  | 'petKind0'
  | 'petKind1'
  | 'petKind2'
  | 'petKind3'
  | 'petKind4'
  | 'petKind5'
  | 'petKind6'
  | 'buffPet';

/** One motion of a sequence: `loop` is an `ANILOOP_*` value (1 play, 4 loop). */
export interface PreviewMotion {
  readonly mti: number;
  readonly loop: 1 | 4;
}

export interface PreviewPreset {
  readonly name: PreviewPresetName;
  /** `perspectiveFovLH(pi / 4, aspect, near, far)`: `fovDiv = 4` (45 degrees), near 0.49, far 512. */
  readonly camera: Omit<CameraState, 'yawDeg' | 'telescope' | 'mode'>;
  /** The model world transform `Scale(scale) . RotY(rotYDeg) . Translate(0, translateY, 0)` (LH row vectors). */
  readonly scale: number;
  readonly translateY: number;
  /** `D3DXMatrixRotationY(D3DXToRadian(rotYDeg))` of the model transform (degrees, D3DX sign); `0` when absent. */
  readonly rotYDeg?: number;
  /** The X and Z of the translation (the head close-ups of the beauty shops): `0` when absent. */
  readonly translateX?: number;
  readonly translateZ?: number;
  /**
   * The translation of a FEMALE mover where the original keys it on the sex (`if( GetSex() == SEX_MALE ) ... else ...` of the beauty shops
   * and the sex-change window): `translateX` / `translateY` / `translateZ` above are the male values.
   */
  readonly female?: {
    readonly translateX?: number;
    readonly translateY: number;
    readonly translateZ?: number;
  };
}

/** The camera of the character slots: eye `(0, 0.5, -3.5)` looking at `(0, 0, 1)`, +Y up, FOV 45 degrees. */
const SLOT_CAMERA = {
  pos: [0, 0.5, -3.5] as Vec3LH,
  lookAt: [0, 0, 1] as Vec3LH,
  up: [0, 1, 0] as Vec3LH,
  fovDiv: 4,
};

/** The view height of the original's orthographic previews: `h = cos(fov / 2) / sin(fov / 2)` with `fov = pi / 4`. */
export const ORTHO_HEIGHT = 1 / Math.tan(Math.PI / 8);
export const CREATE_ORTHO_HEIGHT = ORTHO_HEIGHT;
const ORTHO_EYE_DISTANCE = 500;

/**
 * An orthographic preview view (`D3DXMatrixOrthoLH(w = h * aspect, h, ...)` with `D3DXMatrixLookAtLH(eye, at, up)`) as the
 * limit of a perspective one (the renderer has perspective cameras only): the eye is moved back along the view direction
 * until it is `ORTHO_EYE_DISTANCE` units from the look-at point, the field of view is the one that keeps the view height
 * `ORTHO_HEIGHT` at the look-at plane. At the depth of a character (a few units) the scale error is below 1 percent.
 */
export function orthoCamera(
  eye: Vec3LH,
  lookAt: Vec3LH,
  up: Vec3LH,
): Omit<CameraState, 'yawDeg' | 'telescope' | 'mode'> {
  const d = [lookAt[0] - eye[0], lookAt[1] - eye[1], lookAt[2] - eye[2]] as const;
  const len = Math.hypot(d[0], d[1], d[2]);
  const pos: Vec3LH = [
    lookAt[0] - (d[0] / len) * ORTHO_EYE_DISTANCE,
    lookAt[1] - (d[1] / len) * ORTHO_EYE_DISTANCE,
    lookAt[2] - (d[2] / len) * ORTHO_EYE_DISTANCE,
  ];
  return {
    pos,
    lookAt,
    up,
    /** `fov = pi / fovDiv` with `tan(fov / 2) * distance = height / 2`. */
    fovDiv: Math.PI / (2 * Math.atan(ORTHO_HEIGHT / 2 / ORTHO_EYE_DISTANCE)),
  };
}

const CREATE_EYE_DISTANCE = 500;

/**
 * The character-CREATE camera. `CWndCreateChar::OnDraw` (`WndTitle.cpp:2564-2600`) does not use the slot camera: it uses an
 * ORTHOGRAPHIC projection `D3DXMatrixOrthoLH(w = h * aspect, h = cot(pi/8), 1, 10)` with the eye at `(0, 0, -5)` looking at
 * `(0, 0, 3)` and the model transform `Scale(1.4) . Translate(0, -1.1, 0)` (Plan/45 §5.1 says "the char-select camera": the
 * source wins). The renderer has perspective cameras only, so the orthographic view is reproduced by the limit of a
 * perspective one: the eye moved back to 500 units with the field of view that gives the same view height (`cot(pi/8)` at
 * the look-at plane); at 1.6 units of model depth the perspective error is below 0.5 percent.
 */
const CREATE_CAMERA = {
  pos: [0, 0, -CREATE_EYE_DISTANCE] as Vec3LH,
  lookAt: [0, 0, 3] as Vec3LH,
  up: [0, 1, 0] as Vec3LH,
  /** `fov = pi / fovDiv` with `tan(fov / 2) * distance = height / 2`. */
  fovDiv: Math.PI / (2 * Math.atan(CREATE_ORTHO_HEIGHT / 2 / CREATE_EYE_DISTANCE)),
};

/**
 * The paper doll of the inventory window (`CWndInventory::OnDraw`, `src:_Interface/WndField.cpp:1440-1530`, WP-CFEAT-11,
 * Plan/61 TQ-41-01) and the head portrait of the status window (`CWndStatus::OnDraw`, `WndField.cpp:11150-11260`), as the
 * source states them. Both draw `g_pBipedMesh`-style models through the SAME view: eye `(0, 0.7, -3.5)` looking at
 * `(0, 0, 3)`, +Y up, an ORTHOGRAPHIC projection of height `cot(fov / 2)` (`fov = pi / 4`) and width `height * aspect`, lighting
 * off (`SetLight(FALSE)`, white diffuse and ambient, the hair ambient is the hair colour). They differ in the world
 * transform: the paper doll is `Scale(1.2) . RotY(m_fRot) . Translate(0, -0.6, 0)` (`m_fRot` degrees, dragged with the
 * pointer, `0` when the window opens), the portrait `Scale(6) . RotY(-20) . Translate(0, -7.8, 0)` (the `__VER < 9` critical
 * -HP branch with an extra `RotX(-5)` is not compiled in V15).
 */
export const INVENTORY_PREVIEW_SOURCE = {
  eye: [0, 0.7, -3.5] as Vec3LH,
  lookAt: [0, 0, 3] as Vec3LH,
  up: [0, 1, 0] as Vec3LH,
  /** `D3DX_PI / 4`: the projection height is `cos(fov / 2) / sin(fov / 2)`. */
  fov: Math.PI / 4,
  scale: 1.2,
  rotYDeg: 0,
  translateY: -0.6,
} as const;

export const STATUS_PORTRAIT_PREVIEW_SOURCE = {
  eye: [0, 0.7, -3.5] as Vec3LH,
  lookAt: [0, 0, 3] as Vec3LH,
  up: [0, 1, 0] as Vec3LH,
  fov: Math.PI / 4,
  scale: 6,
  rotYDeg: -20,
  translateY: -7.8,
} as const;

const INVENTORY_CAMERA = orthoCamera(
  INVENTORY_PREVIEW_SOURCE.eye,
  INVENTORY_PREVIEW_SOURCE.lookAt,
  INVENTORY_PREVIEW_SOURCE.up,
);
const PORTRAIT_CAMERA = orthoCamera(
  STATUS_PORTRAIT_PREVIEW_SOURCE.eye,
  STATUS_PORTRAIT_PREVIEW_SOURCE.lookAt,
  STATUS_PORTRAIT_PREVIEW_SOURCE.up,
);

/**
 * The pet windows (WP-UI-33, WP-UI-34). `CWndPetStatus::OnDraw` (`src:_Interface/WndPetSys.cpp:680-770`) looks at the pet with an orthographic view of height
 * `cot(fov / 2)` from `CamPos` at `CamLook` and scales the model by `Scale`, a table of seven kinds; an egg uses eye `(0, 5, -4)`, look-at `(0, 0, 3)`, scale 5. The port
 * has one preset per kind (`petKind0` ... `petKind6`) and `petStatus` for the egg. `CWndBuffPetStatus::OnDraw` frames the eat pet by its event point and radius; the port
 * uses a fixed view of a pet of average size.
 */
const PET_STATUS_CAMERA = orthoCamera([0, 5, -4], [0, 0, 3], [0, 1, 0]);
/** `CWndPetStatus::SetPetCamTable` (`WndPetSys.cpp:225-283`): the eye, the look-at point and the model scale of the seven pet kinds (a hatched pet; an egg uses `petStatus`). */
const PET_KIND_VIEWS: readonly { eye: Vec3LH; look: Vec3LH; scale: number }[] = [
  { eye: [1.2, 4.6, -7.0], look: [-2.4, 3.0, 0.0], scale: 6.0 },
  { eye: [0.7, 3.0, -5.2], look: [-2.2, 2.5, 1.0], scale: 6.0 },
  { eye: [2.0, 2.2, -4.0], look: [-2.2, 1.0, 3.0], scale: 7.0 },
  { eye: [1.4, 4.2, -8.0], look: [-2.2, 2.0, 3.0], scale: 6.0 },
  { eye: [1.4, 6.8, -6.0], look: [-2.0, 6.0, 3.0], scale: 5.5 },
  { eye: [3.0, 6.0, -12.0], look: [-2.0, 3.0, 3.0], scale: 4.5 },
  { eye: [4.0, 3.0, -10.0], look: [-6.0, 6.0, 3.0], scale: 5.0 },
];
function petKindPreset(kind: number, name: PreviewPresetName): PreviewPreset {
  const v = PET_KIND_VIEWS[kind];
  if (!v) throw new Error(`no pet camera for kind ${String(kind)}`);
  return { name, camera: orthoCamera(v.eye, v.look, [0, 1, 0]), scale: v.scale, translateY: 0 };
}
const BUFF_PET_CAMERA = orthoCamera([0.5, 1.8, -3], [-0.24, 0.28, 1], [0, 1, 0]);

export const PREVIEW_PRESETS: Readonly<Record<PreviewPresetName, PreviewPreset>> = {
  charSelect: { name: 'charSelect', camera: SLOT_CAMERA, scale: 1.6, translateY: -1.15 },
  charCreate: { name: 'charCreate', camera: CREATE_CAMERA, scale: 1.4, translateY: -1.1 },
  inventory: {
    name: 'inventory',
    camera: INVENTORY_CAMERA,
    scale: INVENTORY_PREVIEW_SOURCE.scale,
    translateY: INVENTORY_PREVIEW_SOURCE.translateY,
    rotYDeg: INVENTORY_PREVIEW_SOURCE.rotYDeg,
  },
  statusPortrait: {
    name: 'statusPortrait',
    camera: PORTRAIT_CAMERA,
    scale: STATUS_PORTRAIT_PREVIEW_SOURCE.scale,
    translateY: STATUS_PORTRAIT_PREVIEW_SOURCE.translateY,
    rotYDeg: STATUS_PORTRAIT_PREVIEW_SOURCE.rotYDeg,
  },
  // the item model viewer frames the model by its bounds (see `PreviewRenderer`): the transform is the identity
  modelView: { name: 'modelView', camera: SLOT_CAMERA, scale: 1, translateY: 0 },
  // the beauty shops and the sex-change window (`CWndBeautyShop::OnDraw`, `CWndFaceShop::OnDraw`, `CWndChangeSex::OnDraw`,
  // `src:_Interface/WndShop.cpp:1417-2000, 2898-3230`, `WndChangeFace.cpp:90-250`): the camera of the paper doll (eye `(0, 0.7, -3.5)`,
  // look-at `(0, 0, 3)`, orthographic of height `cot(fov / 2)`), lighting off, `MTI_STAND2`. The big previews are `Scale(4.5)` of the
  // whole body at `y = -5.6` (male) / `-5.2` (female); the cards of hair styles are the head `Scale(6)` at `(0.05, -8, -1)` / `(0, -7.5, -1)`,
  // the cards of faces `Scale(7.5)` at `(0.05, -10, -1)` / `(0, -9.5, -1)`. The sex-change window places the OTHER sex's model by the
  // player's sex: `-5.6` for a male player, `-5.8` for a female one.
  beautyHair: {
    name: 'beautyHair',
    camera: INVENTORY_CAMERA,
    scale: 4.5,
    translateY: -5.6,
    female: { translateY: -5.2 },
  },
  hairKind: {
    name: 'hairKind',
    camera: INVENTORY_CAMERA,
    scale: 6,
    translateX: 0.05,
    translateY: -8,
    translateZ: -1,
    female: { translateX: 0, translateY: -7.5, translateZ: -1 },
  },
  faceKind: {
    name: 'faceKind',
    camera: INVENTORY_CAMERA,
    scale: 7.5,
    translateX: 0.05,
    translateY: -10,
    translateZ: -1,
    female: { translateX: 0, translateY: -9.5, translateZ: -1 },
  },
  changeSex: {
    name: 'changeSex',
    camera: INVENTORY_CAMERA,
    scale: 4.5,
    // the subject is the other sex: a male subject is shown to a female player (-5.8), a female subject to a male player (-5.6)
    translateY: -5.8,
    female: { translateY: -5.6 },
  },
  petStatus: { name: 'petStatus', camera: PET_STATUS_CAMERA, scale: 5, translateY: 0 },
  petKind0: petKindPreset(0, 'petKind0'),
  petKind1: petKindPreset(1, 'petKind1'),
  petKind2: petKindPreset(2, 'petKind2'),
  petKind3: petKindPreset(3, 'petKind3'),
  petKind4: petKindPreset(4, 'petKind4'),
  petKind5: petKindPreset(5, 'petKind5'),
  petKind6: petKindPreset(6, 'petKind6'),
  buffPet: { name: 'buffPet', camera: BUFF_PET_CAMERA, scale: 1.5, translateY: 0 },
};

/** `true` for the presets of the appearance windows (they stand on `MTI_STAND2`, lit off, animated only where the window rotates them). */
export function isAppearancePreset(name: PreviewPresetName): boolean {
  return (
    name === 'beautyHair' || name === 'hairKind' || name === 'faceKind' || name === 'changeSex'
  );
}

/**
 * The preset for a subject of `sex` (0 male, 1 female): the sex-keyed translation of the beauty shops resolved (`PREVIEW_PRESETS` holds the
 * male values and the `female` override). Presets that do not key on the sex are returned as they are.
 */
export function resolvePreset(name: PreviewPresetName, sex: 0 | 1): PreviewPreset {
  const p = PREVIEW_PRESETS[name];
  if (sex !== 1 || !p.female) return p;
  return {
    ...p,
    translateX: p.female.translateX ?? 0,
    translateY: p.female.translateY,
    translateZ: p.female.translateZ ?? 0,
  };
}

/** The `CameraState` of a preset (LH, fixed). */
export function presetCamera(p: PreviewPreset): CameraState {
  return { ...p.camera, yawDeg: 0, telescope: 0, mode: 'normal' };
}

/**
 * The camera of a preset seen with the model turned by an extra `RotY(modelYawDeg)` (the paper doll's `m_fRot`): turning the
 * model by `R` is the same image as turning the eye and the look-at point by `R^-1` about the vertical axis through the model
 * origin (the lighting is off, so nothing else depends on the direction).
 */
export function presetCameraTurned(p: PreviewPreset, modelYawDeg: number): CameraState {
  const cam = presetCamera(p);
  if (modelYawDeg === 0) return cam;
  const inv = rotationY((-modelYawDeg * Math.PI) / 180);
  return { ...cam, pos: transformCoord(cam.pos, inv), lookAt: transformCoord(cam.lookAt, inv) };
}

/** The LH model transform of a preset as a row-vector matrix (`Scale . RotY . Translate`). */
export function presetModelMatrixLH(p: PreviewPreset): number[] {
  const rot = rotationY(((p.rotYDeg ?? 0) * Math.PI) / 180);
  return multiply(
    multiply(scaling(p.scale, p.scale, p.scale), rot),
    translation(p.translateX ?? 0, p.translateY, p.translateZ ?? 0),
  );
}

/** The same transform in three.js space (column-major elements). */
export function presetModelMatrixThree(p: PreviewPreset): number[] {
  return mirrorMatrix(presetModelMatrixLH(p));
}

/** "Lighting off": white diffuse and ambient, no fog, no light direction (Plan/41 section 21.2). */
export const PREVIEW_ENV: EnvLight = {
  dif: [1, 1, 1],
  amb: [1, 1, 1],
  fogColor: [0, 0, 0],
  fogStart: 1e6,
  fogEnd: 2e6,
  dirThree: [0, 0, 0],
  night: false,
};

/**
 * The motions a slot asks for (Plan/41 section 21.2): the selected character-select slot plays `MTI_GETUP` (1 play)
 * then `MTI_RUN` (loop; `features.run_on_charselect`, else it stands), the others `MTI_SIT` (1 play) then
 * `MTI_SITSTAND` (loop); character creation stands (`MTI_STAND2` for a female), the paper doll and the portrait stand.
 */
export function previewMotions(
  preset: PreviewPresetName,
  selected: boolean,
  runOnCharSelect = true,
  sex: 0 | 1 = 0,
): PreviewMotion[] {
  // the creation window stands a male on `MTI_STAND` and a female on `MTI_STAND2` (the ponytail to the front)
  if (preset === 'charCreate') return [{ mti: sex === 1 ? MTI_STAND2 : MTI_STAND, loop: 4 }];
  // the beauty shops and the sex-change window load `MTI_STAND2` for both sexes (`LoadMotion( m_pModel, OT_MOVER, nMover, MTI_STAND2 )`)
  if (isAppearancePreset(preset)) return [{ mti: MTI_STAND2, loop: 4 }];
  if (preset !== 'charSelect') return [{ mti: MTI_STAND, loop: 4 }];
  if (selected) {
    return runOnCharSelect
      ? [
          { mti: MTI_GETUP, loop: 1 },
          { mti: MTI_RUN, loop: 4 },
        ]
      : [
          { mti: MTI_GETUP, loop: 1 },
          { mti: MTI_STAND, loop: 4 },
        ];
  }
  return [
    { mti: MTI_SIT, loop: 1 },
    { mti: MTI_SITSTAND, loop: 4 },
  ];
}

/** The preview rate cap (frames per second): the task statement asks for at most 15 fps. */
export const MAX_PREVIEW_FPS = 15;
