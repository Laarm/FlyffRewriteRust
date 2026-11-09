//! The `SimWorld` exports (Plan/31 §17.3). Every export returns `Result<_, JsError>` for malformed input and
//! none can panic (rule X4); the pointers returned by [`SimWorld::take_events`] and
//! [`SimWorld::render_view`] address buffers inside the module's linear memory that stay valid until the
//! next call into the module (the TS side re-creates its views from `memory.buffer` after every call, B4).

use flyff_sim::client::ClientSim;
use flyff_sim::types::SimError;
use wasm_bindgen::prelude::{JsError, wasm_bindgen};

fn js(e: SimError) -> JsError {
    JsError::new(&e.to_string())
}

/// The browser simulation world: the shared deterministic sim plus the client host (command buffer, local
/// control, remote extrapolation, render view).
#[wasm_bindgen]
pub struct SimWorld {
    core: ClientSim,
}

#[wasm_bindgen]
impl SimWorld {
    /// `new SimWorld([flags bits, hit_attr_rule, seed, remote_extrap_cap_frames])`.
    #[wasm_bindgen(constructor)]
    pub fn new(config: &[u32]) -> Result<SimWorld, JsError> {
        Ok(SimWorld {
            core: ClientSim::new(config).map_err(js)?,
        })
    }

    /// `set_world([world_id, mpu, lands_w, lands_h, fly_allowed, housing])`: drops every chunk, world mover,
    /// ctrl collider, decor entry and projectile and starts all lands pending; `world_id == 0` leaves the
    /// world (the other words are ignored, preview movers stay).
    pub fn set_world(&mut self, desc: &[f32]) -> Result<(), JsError> {
        self.core.set_world(desc).map_err(js)
    }

    /// Loads one `FLMT` v1 motion table (the `/gen/motion-table` bytes unchanged).
    pub fn load_motion_table(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.core.load_motion_table(bytes).map_err(js)
    }

    /// Loads one `FLMB` v1 model-bounds file (the `/gen/model-bounds` bytes unchanged).
    pub fn load_model_bounds(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.core.load_model_bounds(bytes).map_err(js)
    }

    /// Loads one `FLYK` v1 ctrl collision-model file (the `/gen/ctrl-models` bytes unchanged): the dynamic
    /// shapes of every ctrl model, once per session. Ctrl colliders already registered are re-posed.
    pub fn load_ctrl_models(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.core.load_ctrl_models(bytes).map_err(js)
    }

    /// Loads one `FLYC` v1 collision chunk (`Pending -> Loaded`; validated again, O(n)).
    pub fn load_chunk(&mut self, lx: u32, lz: u32, bytes: &[u8]) -> Result<(), JsError> {
        self.core.load_chunk(lx, lz, bytes).map_err(js)
    }

    /// Drops the chunk of land `(lx, lz)` (back to pending).
    pub fn unload_chunk(&mut self, lx: u32, lz: u32) {
        self.core.unload_chunk(lx, lz);
    }

    /// Adds one `FSK1` skeleton pack under `skel_id`.
    pub fn add_skeleton(&mut self, skel_id: u32, fsk1: &[u8]) -> Result<(), JsError> {
        self.core.add_skeleton(skel_id, fsk1).map_err(js)
    }

    /// Adds one `FMO1` clip pack under `clip_id` (a `MotionTable` clip id).
    pub fn add_clip(&mut self, clip_id: u32, fmo1: &[u8]) -> Result<(), JsError> {
        self.core.add_clip(clip_id, fmo1).map_err(js)
    }

    /// Applies a command buffer (`{op u16, len u16, payload}` records, Plan/31 §17.4); returns the number of
    /// records accepted. A malformed buffer is an error and changes nothing.
    pub fn apply_commands(&mut self, cmds: &[u8]) -> Result<u32, JsError> {
        self.core.apply_commands(cmds).map_err(js)
    }

    /// The local input record `[buttons, mouse_dx, mouse_dy, level, req_flags]`; applies to the next stepped
    /// frame(s).
    pub fn local_input(&mut self, input: &[i32]) {
        self.core.local_input(input);
    }

    /// `SendActMsg` on the active mover with its synchronous result.
    pub fn local_act(&mut self, msg: u32, p1: i32, p2: i32, p3: i32, p4: i32, p5: i32) -> i32 {
        self.core.local_act(msg, [p1, p2, p3, p4, p5])
    }

    /// Steps up to 60 frames; returns how many were stepped. The local report accumulates over the call.
    pub fn step(&mut self, frames: u32) -> u32 {
        self.core.step(frames)
    }

    /// Pointer to the event buffer `[count, dropped, records...]` (8 words per record); the buffer is
    /// emptied by this call.
    pub fn take_events(&mut self) -> u32 {
        self.core.take_events().as_ptr() as usize as u32 // BOUNDS: wasm32 addresses fit u32
    }

    /// Pointer to the render view (header first, `f32` words, three.js space); rebuilt by every call.
    pub fn render_view(&mut self, alpha: f32) -> u32 {
        self.core.render_view(alpha).as_ptr() as usize as u32 // BOUNDS: wasm32 addresses fit u32
    }

    /// Terrain height at `(x, z)`.
    pub fn land_height(&self, x: f32, z: f32) -> f32 {
        self.core.land_height(x, z)
    }

    /// Height of the highest floor at or below `y` (terrain and objects).
    pub fn full_height(&self, x: f32, y: f32, z: f32) -> f32 {
        self.core.full_height(x, y, z)
    }

    /// `ray_cast([ox,oy,oz, dx,dy,dz, max_t, mask])` -> `[hit (0/1), x, y, z, t, kind (0 none, 1 terrain, 2
    /// static, 3 ctrl), id (u32 bits), nx, ny, nz]` (LH; mask `RAY_TERRAIN 1 | RAY_STATIC 2 | RAY_CTRL 4`).
    pub fn ray_cast(&self, ray: &[f32]) -> Vec<f32> {
        self.core.ray_cast(ray).to_vec()
    }

    /// `camera_check_bound([cam x,y,z, look_at x,y,z])` -> `[crash (0/1), out x,y,z, len]` (Plan/43 §8.4).
    pub fn camera_check_bound(&self, v: &[f32]) -> Vec<f32> {
        self.core.camera_check_bound(v).to_vec()
    }

    /// `height_attr(x, z)`: the attribute class of the cell, `-1` outside the world.
    pub fn height_attr(&self, x: f32, z: f32) -> i32 {
        self.core.height_attr(x, z)
    }

    /// `blink_point([from x,y,z, to x,y,z, range])` -> `[x, y, z, ok]`: the Blinkpool ground point from the
    /// caster toward the cursor point, clamped to `skill_range` and walked back over blocked cells (Plan/43
    /// §7.10); `ok == 0` answers the caster's own position.
    pub fn blink_point(&self, v: &[f32]) -> Vec<f32> {
        self.core.blink_point(v).to_vec()
    }

    /// `target_check(target, range, cone_cos)`: bits `IN_RANGE 1`, `IN_CONE 2`, `AS_BACK_OK 4` for the active
    /// mover and `target` (Plan/43 §10.2); `0` for an unknown target.
    pub fn target_check(&self, target: u32, range: f32, cone_cos: f32) -> u32 {
        self.core
            .target_check(flyff_common::ObjId(target), range, cone_cos)
    }

    /// `range_point(id, x, y, z, r)`: `IsRangeObj(mover, point, r)`; `false` for an unknown mover.
    pub fn range_point(&self, id: u32, x: f32, y: f32, z: f32, r: f32) -> bool {
        self.core.range_point(
            flyff_common::ObjId(id),
            flyff_sim::num::Vec3::new(x, y, z),
            r,
        )
    }

    /// `sample_force_path(id, grip, hit_slow, [f1 x,y,z, f2 x,y,z])` (Plan/31 §13.8; WP-SIM-29): the
    /// sword-trail edge points of the mover's whole swing, `[n, e1 x,y,z, e2 x,y,z, ...]` in model space,
    /// three.js convention. `grip` is 0 right hand, 1 left hand, 2 shield, 3 knuckle; `hit_slow != 0` slows
    /// the clock to 0.1 per step after the hit frame. The live motion clock is not touched; a mover without a
    /// skeleton or clip answers `[0]`.
    pub fn sample_force_path(
        &self,
        id: u32,
        grip: u32,
        hit_slow: u32,
        force: &[f32],
    ) -> Result<Vec<f32>, JsError> {
        self.core
            .sample_force_path(flyff_common::ObjId(id), grip, hit_slow != 0, force)
            .map_err(js)
    }

    /// Restarts the render smoothing of every mover at its simulated state.
    pub fn reset_render_smoothing(&mut self) {
        self.core.reset_render_smoothing();
    }

    /// `[chunks, movers, clips, bytes_static, bytes_clips, decor, ctrls_without_model]`.
    pub fn memory_report(&self) -> Vec<u32> {
        self.core.memory_report().to_vec()
    }
}

impl SimWorld {
    /// The core, for the native tests of the shell.
    pub fn core(&self) -> &ClientSim {
        &self.core
    }

    /// The core, mutably.
    pub fn core_mut(&mut self) -> &mut ClientSim {
        &mut self.core
    }
}

/// Worker export: validates one `FLYC` chunk as fetched (the browser has already removed any
/// `Content-Encoding`) without touching a `SimWorld`; returns a fresh copy of the same bytes (a transferable)
/// or an error naming the violated rule.
#[wasm_bindgen]
pub fn decode_chunk_check(bytes: &[u8]) -> Result<Vec<u8>, JsError> {
    check_chunk(bytes).map_err(js)?;
    Ok(bytes.to_vec())
}

/// The validation of [`decode_chunk_check`] without a `JsError` (natively testable).
pub fn check_chunk(bytes: &[u8]) -> Result<(), SimError> {
    flyff_sim::collision::decode_chunk(bytes).map(|_| ())
}
