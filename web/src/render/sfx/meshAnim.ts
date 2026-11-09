// The animation clock of MESH parts (Plan/41 section 16.4, PL-134; `CModel::FrameMove`, Model.cpp:13-55).
//
// The mesh of a part is a `CModelObject` that the original advances with `FrameMove()` once per `CSfx::Process`, i.e. per instance and
// per 60 Hz tick, on ONE mesh shared by all instances of the file (SfxObj.cpp:160-175): with two instances the animation ran twice
// as fast. The port gives every instance its own clock: the frame is a pure function of the number of steps THAT instance ran.
//
//   frameCur += perSlerp * speed          (speed 1; perSlerp 0.5 = 30 animation frames per second)
//   LOOP: when frameCur > frameMax - 1 the integer part wraps modulo frameMax, the fraction is kept

/** `ANILOOP_LOOP` frame of a mesh after `ticks` steps; `0` for a mesh without an animation (`maxFrame <= 0`). */
export function meshFrame(ticks: number, perSlerp: number, maxFrame: number): number {
  if (maxFrame <= 0 || ticks <= 0) return 0;
  const cur = ticks * perSlerp;
  if (cur <= maxFrame - 1) return cur;
  const f = Math.floor(cur);
  return (f % maxFrame) + (cur - f);
}
