// The flat cloak preview of the wizard (Plan/25 section 6.2 step 3): the texture `Part_{male,female}CloakSLogoNN.dds` (256 x 256 DXT1), decoded by
// the wasm DDS decoder of the decode pool (`dds_bc` = RGBA8, the wiring of `features/session/uiMount.ts`) and drawn into a canvas at an integer
// scale with pixelated rendering. The URL is the RAW DDS (`model/texture/*.dds` has no PNG form). A missing or undecodable texture shows a
// neutral placeholder, never an error page. A 3-D preview is not built (MAY).
import { useEffect, useRef, useState } from 'react';
import { assetUrl } from '../../../assets/paths';
import { createBrowserDecoder } from '../../../ui/skin/images';
import { ddsToPixelImage } from '../../../ui/skin/ddsImage';
import type { PixelImage } from '../../../ui/skin/pixels';
import { Prio } from '../../../workers/pool';
import { decodePool } from '../../world/decodePool';
import { loadAssets } from '../players/gamedata';
import { cloakTexturePath, type CloakSex } from './cloak';
import { G } from './strings';

const decoder = createBrowserDecoder({
  dds: async (bytes) => {
    const pool = await decodePool();
    const r = await pool.submit({ kind: 'dds_bc', bytes, prio: Prio.Visible }).promise;
    return r.blob ? ddsToPixelImage({ meta: r.meta, blob: new Uint8Array(r.blob) }) : null;
  },
});

const decoded = new Map<string, Promise<PixelImage | null>>();

/** Decodes the cloak texture at `path` once (a failed or empty answer is forgotten so that the next try asks again). */
export function loadCloakTexture(path: string): Promise<PixelImage | null> {
  let p = decoded.get(path);
  if (!p) {
    p = (async () => {
      const assets = await loadAssets();
      const url = assets ? assetUrl(assets, path) : null;
      return url === null ? null : await decoder(url);
    })().then(
      (image) => {
        if (image === null) decoded.delete(path);
        return image;
      },
      () => {
        decoded.delete(path);
        return null;
      },
    );
    decoded.set(path, p);
  }
  return p;
}

type State = 'none' | 'loading' | 'ready' | 'missing';

export interface CloakPreviewProps {
  readonly logo: number;
  readonly sex: CloakSex;
  /** Integer scale of the 256 px texture (default 1). */
  readonly scale?: number;
}

export function CloakPreview({ logo, sex, scale = 1 }: CloakPreviewProps) {
  const path = cloakTexturePath(logo, sex);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [loaded, setLoaded] = useState<{ path: string; image: PixelImage | null } | null>(null);
  useEffect(() => {
    if (path === null) return;
    let live = true;
    void loadCloakTexture(path).then((image) => {
      if (live) setLoaded({ path, image });
    });
    return () => {
      live = false;
    };
  }, [path]);
  const image = loaded?.path === path ? loaded.image : null;
  const state: State =
    path === null ? 'none' : loaded?.path !== path ? 'loading' : image ? 'ready' : 'missing';
  useEffect(() => {
    const c = canvas.current;
    if (!c || !image) return;
    const ctx = c.getContext('2d');
    if (!ctx) return;
    c.width = image.width;
    c.height = image.height;
    ctx.putImageData(
      new ImageData(new Uint8ClampedArray(image.data), image.width, image.height),
      0,
      0,
    );
  }, [image]);
  const side = image ? image.width * scale : 256 * scale;
  return (
    <div
      className="gld-cloak"
      data-testid="cloak-preview"
      data-state={state}
      data-path={path ?? ''}
    >
      {state === 'ready' && image ? (
        <canvas
          ref={canvas}
          className="gld-cloak__canvas"
          data-testid="cloak-canvas"
          aria-label={G.create.cloak}
          style={{ width: side, height: image.height * scale }}
        />
      ) : (
        <div
          className="gld-cloak__placeholder"
          style={{ width: side, height: side }}
          role="img"
          aria-label={G.create.cloak}
        >
          <span className="adm-muted adm-small">
            {state === 'none' ? G.create.giveCloakNo : state === 'loading' ? '...' : G.create.cloak}
          </span>
        </div>
      )}
    </div>
  );
}
