//! `.wld` world script (Plan/30 §9.2; `CWorld::OpenWorld`, src:_Common/WorldFile.cpp:56-262; ref/19 §5).
//!
//! The grammar is a `do { GetToken(); match } while (!FINISHED)` loop: every key reads its arguments, every
//! other token (including the values of the keys the original does not parse, `camera`, `bgColor` and
//! `seacloud`) is skipped one at a time.

use flyff_script::{Diagnostics, Script, ScriptCtx};

use super::{check_string_len, open_script, report_truncated};

/// The settings of a `.wld` (Plan/30 §9.2; the `CWorld` constructor defaults apply to absent keys). Ids are
/// plain `u32`: `flyff-data` re-exports the type with `WorldId`/`MusicId` as `u32` aliases (14 §6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct WldSettings {
    /// `size w , h`: land counts in X and Z (default (0, 0)).
    pub size: (u32, u32),
    /// `indoor` (default false).
    pub indoor: bool,
    /// `ambient`: hex colour (default `0xFF808080`; a file value is stored as written, e.g. `0x808080`).
    pub ambient: u32,
    /// `fly` (default **true**).
    pub fly: bool,
    /// `bgm`: music id (default 0).
    pub bgm: u32,
    /// `pkmode`: default `RA_*` region attribute flags of the world (default 0).
    pub pk_mode: u32,
    /// `diffuse`: hex colour (default 0).
    pub diffuse: u32,
    /// `lightDir` (default (0, 0, 0)).
    pub light_dir: [f32; 3],
    /// `fogSetting start end density` (default (70, 400, 0)).
    pub fog: (f32, f32, f32),
    /// `MPU`: metres per unit (default 4: `g_MPU = OLD_MPU` when the key is absent).
    pub mpu: u32,
    /// `revival <world> "<key>"`: world id (0 = `WI_WORLD_NONE`).
    pub revival_world: u32,
    /// The revival key (default empty).
    pub revival_key: String,
    /// `sky`: day, evening, night texture; `None` = `default`.
    pub sky: [Option<String>; 3],
    /// `cloud`: day, evening, night texture; `None` = `default`.
    pub cloud: [Option<String>; 3],
    /// `sun`: texture, `None` = `default`.
    pub sun: Option<String>,
    /// `moon`: texture, `None` = `default`. FIX of the original, which stored it into the sun slot.
    pub moon: Option<String>,
    /// `true` when the file has a `moon` key: the original mishandled it (the value of a non-default `moon`
    /// went into the sun slot, WorldFile.cpp:185-193). No corpus file is affected (all say `default`).
    pub moon_into_sun_bug: bool,
    /// `use24light`: the loader (14) reads `<path>.24l` into `light24` when set.
    pub use24light: bool,
    /// The `.24l` rows; filled by the loader, never by [`parse_wld`].
    pub light24: Option<Vec<[f32; 6]>>,
}

impl Default for WldSettings {
    fn default() -> Self {
        WldSettings {
            size: (0, 0),
            indoor: false,
            ambient: 0xFF80_8080,
            fly: true,
            bgm: 0,
            pk_mode: 0,
            diffuse: 0,
            light_dir: [0.0; 3],
            fog: (70.0, 400.0, 0.0),
            mpu: 4,
            revival_world: 0,
            revival_key: String::new(),
            sky: [None, None, None],
            cloud: [None, None, None],
            sun: None,
            moon: None,
            moon_into_sun_bug: false,
            use24light: false,
            light24: None,
        }
    }
}

/// The keys the original parses; everything else is skipped.
#[derive(Clone, Copy)]
enum Key {
    Size,
    Ambient,
    Revival,
    Indoor,
    Fly,
    Bgm,
    PkMode,
    Diffuse,
    LightDir,
    FogSetting,
    Mpu,
    Sky,
    Cloud,
    Sun,
    Moon,
    Use24Light,
}

fn key_of(token: &str) -> Option<Key> {
    Some(match token {
        "size" => Key::Size,
        "ambient" => Key::Ambient,
        "revival" => Key::Revival,
        "indoor" => Key::Indoor,
        "fly" => Key::Fly,
        "bgm" => Key::Bgm,
        "pkmode" => Key::PkMode,
        "diffuse" => Key::Diffuse,
        "lightDir" => Key::LightDir,
        "fogSetting" => Key::FogSetting,
        "MPU" => Key::Mpu,
        "sky" => Key::Sky,
        "cloud" => Key::Cloud,
        "sun" => Key::Sun,
        "moon" => Key::Moon,
        "use24light" => Key::Use24Light,
        _ => return None,
    })
}

/// Reads the arguments of `key` into `w`.
fn read_key(s: &mut Script<'_>, key: Key, w: &mut WldSettings) {
    match key {
        Key::Size => {
            let width = s.get_number();
            s.get_token(); // the comma
            let height = s.get_number();
            w.size = (width, height);
        }
        Key::Ambient => w.ambient = s.get_hex(),
        Key::Revival => {
            w.revival_world = s.get_number();
            s.get_token();
            w.revival_key = s.token().to_owned();
            let key = w.revival_key.clone();
            check_string_len(s, "revival key", &key);
        }
        Key::Indoor => w.indoor = s.get_number() != 0,
        Key::Fly => w.fly = s.get_number() != 0,
        Key::Bgm => w.bgm = s.get_number(),
        Key::PkMode => w.pk_mode = s.get_number(),
        Key::Diffuse => w.diffuse = s.get_hex(),
        Key::LightDir => w.light_dir = [s.get_float(), s.get_float(), s.get_float()],
        Key::FogSetting => w.fog = (s.get_float(), s.get_float(), s.get_float()),
        Key::Mpu => w.mpu = s.get_number(),
        Key::Sky => w.sky = read_three_names(s),
        Key::Cloud => w.cloud = read_three_names(s),
        Key::Sun => w.sun = read_name(s),
        Key::Moon => {
            w.moon = read_name(s);
            w.moon_into_sun_bug = true;
        }
        Key::Use24Light => w.use24light = s.get_number() != 0,
    }
}

/// One texture name; `default` means "keep the built-in one".
fn read_name(s: &mut Script<'_>) -> Option<String> {
    s.get_token();
    (!s.is(DEFAULT_NAME)).then(|| s.token().to_owned())
}

/// Day, evening and night names of `sky`/`cloud`.
fn read_three_names(s: &mut Script<'_>) -> [Option<String>; 3] {
    let day = read_name(s);
    let evening = read_name(s);
    let night = read_name(s);
    [day, evening, night]
}

/// `DEFAULT_FNAME`: the placeholder for "no world-specific texture".
const DEFAULT_NAME: &str = "default";

/// Parses a `.wld` (Plan/30 §9.2). `file` is the name used in diagnostics. A key whose arguments are cut off
/// by the end of the file is dropped (`WorldTextTruncated`) and the defaults stay.
pub fn parse_wld(text: &str, file: &str, ctx: &ScriptCtx, diags: &mut Diagnostics) -> WldSettings {
    let mut w = WldSettings::default();
    let mut s = open_script(text, file, ctx, diags);
    loop {
        s.get_token();
        if let Some(key) = key_of(s.token()) {
            let mut next = w.clone();
            read_key(&mut s, key, &mut next);
            if s.finished() {
                // The arguments ran into the end of the file: the original read zeros; the key is dropped.
                report_truncated(&mut s, "a .wld key");
                break;
            }
            w = next;
        }
        if s.finished() {
            break;
        }
    }
    w
}
