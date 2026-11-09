// The job icon (`Icon_JobClassTextures.dds`, one sprite per job id, rects in its `.inc` of the `ui-atlases` bundle). Without the bundle or
// the image (before they load, a server without assets) the job is shown as its initial in a round badge.
import { useMemo } from 'react';
import { spriteRect } from '../../../ui/skin/atlas';
import { useAssetUrl, useBundle } from './gamedata';
import { jobName } from './jobs';

const ATLAS = 'icon/icon_jobclasstextures.inc';

export function JobIcon({ job, size = 24 }: { readonly job: number; readonly size?: number }) {
  const atlases = useBundle('ui-atlases');
  const atlas = atlases.data?.atlases[ATLAS];
  const sprite = atlas?.sprites[job];
  const url = useAssetUrl(atlas?.texture ?? ATLAS.replace('.inc', '.dds'));
  const css = useMemo(() => {
    if (!atlas || !sprite || !url) return null;
    const [sx, sy, w, h] = spriteRect(sprite, atlas.size[0], atlas.size[1]);
    const scale = Math.min(size / w, size / h);
    return {
      width: Math.round(w * scale),
      height: Math.round(h * scale),
      backgroundImage: `url("${url}")`,
      backgroundSize: `${String(Math.round(atlas.size[0] * scale))}px ${String(Math.round(atlas.size[1] * scale))}px`,
      backgroundPosition: `${String(-Math.round(sx * scale))}px ${String(-Math.round(sy * scale))}px`,
      backgroundRepeat: 'no-repeat',
      imageRendering: 'auto' as const,
    };
  }, [atlas, sprite, url, size]);
  const name = jobName(job);
  if (!css) {
    return (
      <span
        className="plr-job plr-job--text"
        style={{ width: size, height: size }}
        title={name}
        role="img"
        aria-label={name}
      >
        {name.charAt(0)}
      </span>
    );
  }
  return <span className="plr-job" style={css} title={name} role="img" aria-label={name} />;
}
