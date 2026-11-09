//! The render view (`render_view(alpha)`, Plan/31 §17.7; WP-SIM-14): one contiguous `f32` buffer with a
//! 32-word header, the mover records, the attachment records, the projectile records, the (empty) decor
//! area, the local report and the bone palettes, laid out by [`crate::wire::view`].
//!
//! Matrices are written in three.js space (`S·M·S`, `S = diag(1, 1, -1, 1)`) as three vec4 rows of the
//! transposed affine matrix ([`crate::pose::palette::render_rows`], the single definition shared with the palettes).
//! The palette area is zero-padded to whole texture rows (512 matrices = 6144 floats), so uploading
//! `palette_rows` full rows never reads past the buffer. The buffer is rebuilt on every call; its address can
//! change after any call into the module (B4).

use super::{ClientMover, ClientSim, decor};
use crate::bounds::radius;
use crate::collision::world::{CollisionWorld, LandSlot};
use crate::mover::SimMover;
use crate::num::{Vec3, cosf, sinf, to_radian};
use crate::pose::palette::render_rows;
use crate::pose::quat::{mat43_from_44, mat44_from_43, mul44};
use crate::pose::{
    ATTACH_FLOATS, MATRIX_FLOATS, Mat43, PALETTE_ROW_FLOATS, PoseLibrary, PoseState, palette_rows,
};
use crate::remote;
use crate::types::MoverKind;
use crate::wire::view as v;

/// The render view buffer. The view starts at a 16-byte aligned address (the allocator only promises 8), so
/// the 16-byte aligned `palette_off` of the header is aligned in absolute terms too.
#[derive(Clone, Debug, Default)]
pub struct ViewBuffer {
    data: Vec<f32>,
    /// Index of the first word of the view inside `data` (0..=3).
    start: usize,
    len: usize,
}

impl ViewBuffer {
    /// An empty buffer (the first `render_view` fills it).
    pub fn new() -> ViewBuffer {
        ViewBuffer::default()
    }

    /// The words of the last view (header first).
    pub fn as_slice(&self) -> &[f32] {
        self.data
            .get(self.start..self.start + self.len)
            .unwrap_or_default()
    }

    /// Zeroes the storage for a view of `len` words and aligns its start.
    fn prepare(&mut self, len: usize) -> &mut [f32] {
        self.data.clear();
        self.data.resize(len + 3, 0.0);
        let addr = self.data.as_ptr() as usize;
        self.start = ((16 - addr % 16) % 16) / 4;
        self.len = len;
        self.data
            .get_mut(self.start..self.start + len)
            .unwrap_or_default()
    }
}

fn put(buf: &mut [f32], i: usize, x: f32) {
    if let Some(s) = buf.get_mut(i) {
        *s = x;
    }
}

fn put_bits(buf: &mut [f32], i: usize, x: u32) {
    put(buf, i, f32::from_bits(x));
}

/// `Scale(s) · RotYawPitchRoll(-yaw, -pitch, 0) · Translate(pos)` of `CObj::UpdateMatrix` (Plan/31 §15.1,
/// ref/18 §11.2 item 9) as the 4x3 left-handed matrix; `D3DXMatrixRotationYawPitchRoll` with `roll = 0`.
pub fn world_matrix_lh(pos: Vec3, yaw_deg: f32, pitch_deg: f32, scale: f32) -> Mat43 {
    let (sy, cy) = (sinf(to_radian(-yaw_deg)), cosf(to_radian(-yaw_deg)));
    let (sp, cp) = (sinf(to_radian(-pitch_deg)), cosf(to_radian(-pitch_deg)));
    [
        scale * cy,
        0.0,
        scale * -sy,
        scale * (sp * sy),
        scale * cp,
        scale * (sp * cy),
        scale * (cp * sy),
        scale * -sp,
        scale * (cp * cy),
        pos.x,
        pos.y,
        pos.z,
    ]
}

/// The three.js-space rows of a mover world matrix.
pub fn world_rows(pos: Vec3, yaw_deg: f32, pitch_deg: f32, scale: f32) -> [f32; MATRIX_FLOATS] {
    render_rows(&world_matrix_lh(pos, yaw_deg, pitch_deg, scale))
}

/// What a mover record needs besides the simulation state.
struct Item<'a> {
    sim: &'a SimMover,
    pose: &'a mut PoseState,
    skel_id: u32,
    visible: bool,
    want_attach: bool,
    preview: bool,
}

fn pending_land(world: &CollisionWorld, world_active: bool, pos: Vec3) -> bool {
    if !world_active {
        return false;
    }
    let span = (crate::collision::chunk::MAP_SIZE as f32) * world.mpu as f32;
    if !(pos.x >= 0.0 && pos.z >= 0.0) || span <= 0.0 {
        return false;
    }
    let lx = (pos.x / span) as u32; // BOUNDS: non-negative finite, saturating cast
    let lz = (pos.z / span) as u32; // BOUNDS: see above
    matches!(world.slot(lx, lz), Some(LandSlot::Pending))
}

impl ClientSim {
    /// `render_view(alpha)`: rebuilds the buffer and returns it; `alpha` in `[0, 1]` interpolates the smoothed
    /// positions and yaws between the previous and the current frame (PL-009). Visible movers with a loaded
    /// skeleton get their pose evaluated and their palette (and, when wanted, attachments) written.
    pub fn render_view(&mut self, alpha: f32) -> &[f32] {
        let alpha = if alpha.is_finite() {
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let frame = self.frame;
        let world_active = self.world_id != 0;
        let ClientSim {
            world,
            poses,
            movers,
            previews,
            projectiles,
            control,
            view,
            decor,
            motions,
            ..
        } = self;
        let mut items: Vec<Item<'_>> = Vec::with_capacity(movers.len() + previews.len());
        for m in movers.iter_mut() {
            let ClientMover {
                sim,
                pose,
                skel_id,
                visible,
                want_attach,
                ..
            } = m;
            items.push(Item {
                sim,
                pose,
                skel_id: *skel_id,
                visible: *visible,
                want_attach: *want_attach,
                preview: false,
            });
        }
        for p in previews.iter_mut() {
            items.push(Item {
                sim: &p.sim,
                pose: &mut p.pose,
                skel_id: p.skel_id,
                visible: p.visible,
                want_attach: p.want_attach,
                preview: true,
            });
        }
        // layout
        let skel_bones = |it: &Item<'_>, poses: &PoseLibrary| -> Option<usize> {
            if it.visible && it.skel_id != 0 {
                poses.skeleton(it.skel_id).map(|s| s.bones.len())
            } else {
                None
            }
        };
        let n = items.len();
        let mut matrices = 0usize;
        let mut attach_count = 0usize;
        for it in &items {
            if let Some(b) = skel_bones(it, poses) {
                matrices += b;
                if it.want_attach {
                    attach_count += 1;
                }
            }
        }
        let movers_off = v::HEADER_WORDS;
        let attach_off = movers_off + n * v::MOVER_STRIDE;
        let proj_off = attach_off + attach_count * v::ATTACH_STRIDE;
        let proj_n = projectiles.len();
        let decor_off = proj_off + proj_n * v::PROJ_STRIDE;
        let decor_plan = decor.plan(poses, motions);
        let decor_n = decor_plan.len();
        matrices += decor_plan.iter().flatten().sum::<usize>();
        let report_off = decor_off + decor_n * v::DECOR_STRIDE;
        let palette_off = (report_off + v::REPORT_FLOATS).next_multiple_of(4);
        let palette_floats = matrices * MATRIX_FLOATS;
        let rows = palette_rows(palette_floats);
        let total = palette_off + rows * PALETTE_ROW_FLOATS;
        let buf = view.prepare(total);

        // the drawn world matrix of every mover (LH): interpolated, heel included
        let worlds_lh: Vec<Mat43> = items
            .iter()
            .map(|it| {
                let m = it.sim;
                let (pos, yaw, pitch) = if it.preview {
                    (m.pos, m.angle, m.angle_x)
                } else {
                    let (p, y) = remote::interpolate(m, alpha);
                    (p, y, m.render.scr_angle_x)
                };
                let heel =
                    if !it.preview && m.params.kind == MoverKind::Player && !m.act.flags.fly() {
                        v::HEEL
                    } else {
                        0.0
                    };
                world_matrix_lh(
                    Vec3::new(pos.x, pos.y + heel, pos.z),
                    yaw,
                    pitch,
                    m.params.scale,
                )
            })
            .collect();
        let mut matrix_cursor = 0usize;
        let mut attach_cursor = 0usize;
        for (i, it) in items.iter_mut().enumerate() {
            let rec = movers_off + i * v::MOVER_STRIDE;
            let m = it.sim;
            let mut flags = 0u32;
            let mut palette_index = v::NO_INDEX;
            let mut bone_count = 0u32;
            let mut attach_index = v::NO_INDEX;
            if it.visible {
                flags |= v::F_VISIBLE;
            }
            if let Some(skel) = poses
                .skeleton(it.skel_id)
                .filter(|_| it.visible && it.skel_id != 0)
            {
                it.pose.update_motion(skel, poses, &m.motion);
                let at = palette_off + matrix_cursor * MATRIX_FLOATS;
                let written = it
                    .pose
                    .write_palette(skel, buf.get_mut(at..).unwrap_or_default());
                palette_index = matrix_cursor as u32; // BOUNDS: at most a few thousand matrices
                bone_count = written as u32; // BOUNDS: at most 32,767 bones
                matrix_cursor += skel.bones.len();
                if it.pose.has_pose() {
                    flags |= v::F_HAS_POSE;
                }
                if it.want_attach {
                    let a = attach_off + attach_cursor * v::ATTACH_STRIDE;
                    if let Some(out) = buf.get_mut(a..a + ATTACH_FLOATS)
                        && it.pose.write_attachments(skel, out)
                    {
                        attach_index = attach_cursor as u32; // BOUNDS: at most a few hundred
                        flags |= v::F_HAS_ATTACH;
                    }
                    attach_cursor += 1;
                }
            }
            if m.act.flags.fly() {
                flags |= v::F_FLY;
            }
            if m.act.state.is_die() {
                flags |= v::F_DEAD;
            }
            if m.is_active {
                flags |= v::F_ACTIVE;
            }
            if m.lagging {
                flags |= v::F_LAGGING;
            }
            if it.preview {
                flags |= v::F_PREVIEW;
            } else if pending_land(world, world_active, m.pos) {
                flags |= v::F_PENDING_LAND;
            }
            let rows = worlds_lh
                .get(i)
                .map(render_rows)
                .unwrap_or([0.0; MATRIX_FLOATS]);
            for (k, r) in rows.iter().enumerate() {
                put(buf, rec + v::M_WORLD + k, *r);
            }
            put_bits(buf, rec + v::M_ID, m.id.0);
            put_bits(buf, rec + v::M_FLAGS, flags);
            put_bits(buf, rec + v::M_PALETTE_INDEX, palette_index);
            put_bits(buf, rec + v::M_BONE_COUNT, bone_count);
            put_bits(buf, rec + v::M_ATTACH_INDEX, attach_index);
            put_bits(buf, rec + v::M_MOTION, m.motion.id);
            put_bits(buf, rec + v::M_MOTION_RESOLVED, m.motion.resolved);
            put(buf, rec + v::M_FRAME_CUR, m.motion.clock.frame_cur);
            put_bits(buf, rec + v::M_STATE, m.act.state.bits());
            put_bits(buf, rec + v::M_STATE_FLAGS, m.act.flags.bits());
            put(buf, rec + v::M_POS, m.pos.x);
            put(buf, rec + v::M_POS + 1, m.pos.y);
            put(buf, rec + v::M_POS + 2, m.pos.z);
            put(buf, rec + v::M_ANGLE, m.angle);
            put(
                buf,
                rec + v::M_ROLL,
                if m.is_active && !it.preview {
                    control.roll
                } else {
                    0.0
                },
            );
            put(
                buf,
                rec + v::M_RADIUS,
                radius(&m.params.bounds, m.params.scale),
            );
            put_bits(
                buf,
                rec + v::M_CLIP_ID,
                m.motion.clip.unwrap_or(v::NO_INDEX),
            );
            put(buf, rec + v::M_CURRENT_HEIGHT, m.act.current_height);
        }
        // projectiles: `0 id`, `1 kind`, `2 flags`, `3-5 pos` (interpolated between the last two steps), `6-8
        // dir` (unit, LH), `9 age`, `10-12` the logical (not interpolated) position, `13-15` reserved
        for (i, q) in projectiles.iter().enumerate() {
            let rec = proj_off + i * v::PROJ_STRIDE;
            let p = &q.p;
            let mut flags = 0u32;
            if !p.done {
                flags |= v::P_ALIVE;
            } else if p.hit {
                flags |= v::P_HIT;
            } else {
                flags |= v::P_EXPIRED;
            }
            let shown = if p.done {
                p.pos
            } else {
                Vec3::new(
                    q.prev_pos.x + (p.pos.x - q.prev_pos.x) * alpha,
                    q.prev_pos.y + (p.pos.y - q.prev_pos.y) * alpha,
                    q.prev_pos.z + (p.pos.z - q.prev_pos.z) * alpha,
                )
            };
            put_bits(buf, rec, p.id);
            put_bits(buf, rec + 1, u32::from(p.kind.as_u8()));
            put_bits(buf, rec + 2, flags);
            put(buf, rec + 3, shown.x);
            put(buf, rec + 4, shown.y);
            put(buf, rec + 5, shown.z);
            put(buf, rec + 6, p.dir.x);
            put(buf, rec + 7, p.dir.y);
            put(buf, rec + 8, p.dir.z);
            put_bits(buf, rec + 9, p.age);
            put(buf, rec + 10, p.pos.x);
            put(buf, rec + 11, p.pos.y);
            put(buf, rec + 12, p.pos.z);
        }
        // decor and ctrl entries: `0 id`, `1 flags`, `2 palette_index`, `3 matrix_count`, `4 frame_cur`,
        // `5-16` the world matrix (three.js rows); the poses go to the palette after the mover matrices
        {
            let mut plan = decor_plan.iter();
            let mut rec = decor_off;
            let (entries, ctrl_entries) = decor.parts_mut();
            for e in entries {
                plan.next();
                let mut flags = 0u32;
                let mut palette_index = v::NO_INDEX;
                let mut count = 0u32;
                if e.visible {
                    flags |= v::DF_VISIBLE;
                }
                let at = palette_off + matrix_cursor * MATRIX_FLOATS;
                let w = e.write_pose(poses, motions, buf.get_mut(at..).unwrap_or_default());
                if w.has_pose {
                    flags |= v::DF_HAS_POSE;
                    palette_index = matrix_cursor as u32; // BOUNDS: a few thousand matrices
                    count = w.count as u32; // BOUNDS: at most 32,767
                    matrix_cursor += w.count;
                }
                if e.skinned() {
                    flags |= v::DF_SKINNED;
                }
                if e.ended() {
                    flags |= v::DF_ENDED;
                }
                // static: `local`; attached: `local · attach · parent world`
                let world44 = match e.kind {
                    decor::DecorKind::Static => mat44_from_43(&e.local),
                    decor::DecorKind::Attached => {
                        let parent = items.iter().position(|it| it.sim.id == e.parent);
                        let on = parent.and_then(|pi| {
                            let it = items.get(pi)?;
                            let att = e.attach_on(poses, it.skel_id, it.pose)?;
                            let pw = mat44_from_43(worlds_lh.get(pi)?);
                            Some(mul44(&mat44_from_43(&e.local), &mul44(&att, &pw)))
                        });
                        on.unwrap_or_else(|| mat44_from_43(&e.local))
                    }
                };
                put_bits(buf, rec + v::D_ID, e.id);
                put_bits(buf, rec + v::D_FLAGS, flags);
                put_bits(buf, rec + v::D_PALETTE_INDEX, palette_index);
                put_bits(buf, rec + v::D_MATRIX_COUNT, count);
                put(buf, rec + v::D_FRAME_CUR, e.frame());
                for (k, r) in render_rows(&mat43_from_44(&world44)).iter().enumerate() {
                    put(buf, rec + v::D_WORLD + k, *r);
                }
                rec += v::DECOR_STRIDE;
            }
            for e in ctrl_entries {
                plan.next();
                let mut flags = v::DF_CTRL;
                let mut palette_index = v::NO_INDEX;
                let mut count = 0u32;
                if e.visible {
                    flags |= v::DF_VISIBLE;
                }
                let at = palette_off + matrix_cursor * MATRIX_FLOATS;
                let w = e.write_pose(poses, motions, buf.get_mut(at..).unwrap_or_default());
                if w.has_pose {
                    flags |= v::DF_HAS_POSE;
                    palette_index = matrix_cursor as u32; // BOUNDS: a few thousand matrices
                    count = w.count as u32; // BOUNDS: at most 32,767
                    matrix_cursor += w.count;
                }
                if e.clock.ended() {
                    flags |= v::DF_ENDED;
                }
                put_bits(buf, rec + v::D_ID, e.id.0);
                put_bits(buf, rec + v::D_FLAGS, flags);
                put_bits(buf, rec + v::D_PALETTE_INDEX, palette_index);
                put_bits(buf, rec + v::D_MATRIX_COUNT, count);
                put(buf, rec + v::D_FRAME_CUR, e.clock.frame());
                for (k, r) in render_rows(&mat43_from_44(&e.world)).iter().enumerate() {
                    put(buf, rec + v::D_WORLD + k, *r);
                }
                rec += v::DECOR_STRIDE;
            }
        }
        // the report of the last `step` call is carried over unchanged
        if let Some(out) = buf.get_mut(report_off..report_off + v::REPORT_FLOATS) {
            control.report.write_floats(out);
        }
        // header
        let h = |buf: &mut [f32], i: usize, x: u32| put_bits(buf, i, x);
        let u = |x: usize| x as u32; // BOUNDS: every offset is far below 2^32 floats
        h(buf, v::H_MAGIC, v::MAGIC);
        h(buf, v::H_VERSION, v::VERSION);
        h(buf, v::H_FRAME_LO, frame as u32); // BOUNDS: low word
        h(buf, v::H_FRAME_HI, (frame >> 32) as u32); // BOUNDS: high word
        h(buf, v::H_MOVER_COUNT, u(n));
        h(buf, v::H_MOVER_STRIDE, u(v::MOVER_STRIDE));
        h(buf, v::H_MOVERS_OFF, u(movers_off));
        h(buf, v::H_PALETTE_OFF, u(palette_off));
        h(buf, v::H_PALETTE_FLOATS, u(palette_floats));
        h(buf, v::H_ATTACH_OFF, u(attach_off));
        h(buf, v::H_ATTACH_STRIDE, u(v::ATTACH_STRIDE));
        h(buf, v::H_PROJ_COUNT, u(proj_n));
        h(buf, v::H_PROJ_STRIDE, u(v::PROJ_STRIDE));
        h(buf, v::H_PROJ_OFF, u(proj_off));
        h(buf, v::H_REPORT_OFF, u(report_off));
        h(buf, v::H_REPORT_FLOATS, u(v::REPORT_FLOATS));
        h(buf, v::H_DECOR_COUNT, u(decor_n));
        h(buf, v::H_DECOR_STRIDE, u(v::DECOR_STRIDE));
        h(buf, v::H_DECOR_OFF, u(decor_off));
        h(buf, v::H_PALETTE_ROWS, u(rows));
        view.as_slice()
    }
}

/// A parsed view header (tests, the TS reader mirrors it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewHeader {
    /// Number of mover records.
    pub mover_count: usize,
    /// Offset of the mover area.
    pub movers_off: usize,
    /// Offset of the palette area.
    pub palette_off: usize,
    /// Floats of palette data.
    pub palette_floats: usize,
    /// Offset of the attachment area.
    pub attach_off: usize,
    /// Offset of the report area.
    pub report_off: usize,
    /// Palette texture rows.
    pub palette_rows: usize,
    /// Local frame counter.
    pub frame: u64,
}

impl ViewHeader {
    /// Parses and validates the header of a view buffer (magic, version, strides, offsets inside the
    /// buffer, palette padding).
    pub fn parse(buf: &[f32]) -> Option<ViewHeader> {
        let w = |i: usize| buf.get(i).map(|f| f.to_bits());
        if w(v::H_MAGIC)? != v::MAGIC || w(v::H_VERSION)? != v::VERSION {
            return None;
        }
        if w(v::H_MOVER_STRIDE)? as usize != v::MOVER_STRIDE
            || w(v::H_ATTACH_STRIDE)? as usize != v::ATTACH_STRIDE
            || w(v::H_REPORT_FLOATS)? as usize != v::REPORT_FLOATS
        {
            return None;
        }
        let h = ViewHeader {
            mover_count: w(v::H_MOVER_COUNT)? as usize,
            movers_off: w(v::H_MOVERS_OFF)? as usize,
            palette_off: w(v::H_PALETTE_OFF)? as usize,
            palette_floats: w(v::H_PALETTE_FLOATS)? as usize,
            attach_off: w(v::H_ATTACH_OFF)? as usize,
            report_off: w(v::H_REPORT_OFF)? as usize,
            palette_rows: w(v::H_PALETTE_ROWS)? as usize,
            frame: u64::from(w(v::H_FRAME_LO)?) | (u64::from(w(v::H_FRAME_HI)?) << 32),
        };
        let end = h
            .palette_off
            .checked_add(h.palette_rows.checked_mul(PALETTE_ROW_FLOATS)?)?;
        (end == buf.len()
            && h.palette_off.is_multiple_of(4)
            && h.palette_rows == palette_rows(h.palette_floats)
            && h.movers_off + h.mover_count * v::MOVER_STRIDE <= buf.len())
        .then_some(h)
    }
}
