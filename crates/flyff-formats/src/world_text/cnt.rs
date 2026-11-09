//! `.wld.cnt` continent polygons (Plan/30 §9.5; server `CContinent::Init`, src:_Common/Continent.cpp:30-104;
//! client `CWorld::ReadFileContinent`, src:_Common/WorldEnvironment.cpp:633-740; ref/19 §11).
//!
//! A neutral parse: both original readers are reproduced by consumer helpers (the server keeps only
//! `C_useRealData 1` blocks and files the towns by `C_id`, the client keeps only `C_useEnvir 1` blocks).
//! Every block starts fresh (FIX of the client's field carry-over and the server's `TOWN` leak: no data is
//! affected, every environment block sets all its keys and all `TOWN` values are 0).

use flyff_common::math::ftoi_x86;
use flyff_script::{Diagnostics, Script, ScriptCtx};

use super::{open_script, report_truncated};

/// A parsed `.wld.cnt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CntFile {
    /// `WORLD <file>.wld` (the 14 stub files contain only this line).
    pub world: Option<String>,
    /// The `Continent BEGIN` ... `Continent END` blocks in file order.
    pub continents: Vec<ContinentDef>,
}

/// One continent block (= 14 §6.9 `ContinentDef`). Absent keys keep the fresh-block defaults (false, empty,
/// 0, `None`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContinentDef {
    /// `TOWN`: the polygon is a town (the server keeps it in `m_MapContTown`).
    pub town: bool,
    /// `C_useEnvir`: the polygon overrides lighting/fog/sky/weather on the client.
    pub use_envir: bool,
    /// `C_useRealData`: a gameplay continent for the server (a `0` block is client visuals only).
    pub use_real_data: bool,
    /// `C_name`.
    pub name: String,
    /// `C_id` (`ContinentDef.h`: 1 Flaris, 2 SaintMorning, 3 Ricis, 4 Estia, 241 Darkon12, 242 Darkon3,
    /// 243 Harmonin).
    pub id: u32,
    /// `C_ambient`: 3 floats in 0..1.
    pub ambient: Option<[f32; 3]>,
    /// `C_diffuse`: 3 floats in 0..1.
    pub diffuse: Option<[f32; 3]>,
    /// `C_fogstartend`: start and end.
    pub fog: Option<(f32, f32)>,
    /// `C_weather`: `WEATHER_*` (2 = rain).
    pub weather: Option<u32>,
    /// `C_sky <use> <file in Weather/>`.
    pub sky: Option<(bool, String)>,
    /// `C_cloud <use> <file in Weather/>`.
    pub cloud: Option<(bool, String)>,
    /// `VERTEX x y z` as written (the polygon is not closed here).
    pub vertices: Vec<[f32; 3]>,
}

impl ContinentDef {
    /// The vertices with the first one appended (both readers closed the polygon at `END`); empty stays empty.
    pub fn closed_vertices(&self) -> Vec<[f32; 3]> {
        let mut v = self.vertices.clone();
        if let Some(first) = self.vertices.first() {
            v.push(*first);
        }
        v
    }

    /// The closed polygon as the server stored it: `((int)x, (int)z)` per vertex (`CPoint`, x87 `(int)` cast;
    /// `y` is ignored by the containment test).
    pub fn server_points(&self) -> Vec<(i32, i32)> {
        self.closed_vertices()
            .iter()
            .map(|v| (ftoi_x86(v[0]), ftoi_x86(v[2])))
            .collect()
    }
}

fn vec3(s: &mut Script<'_>) -> [f32; 3] {
    [s.get_float(), s.get_float(), s.get_float()]
}

/// `<flag> <name>` of `C_sky`/`C_cloud`.
fn flag_and_name(s: &mut Script<'_>) -> (bool, String) {
    let used = s.get_number() != 0;
    s.get_token();
    (used, s.token().to_owned())
}

/// The block keys the original readers parse (`C_sun`/`C_moon` and everything else are skipped).
#[derive(Clone, Copy)]
enum Key {
    Town,
    UseEnvir,
    UseRealData,
    Id,
    Weather,
    Name,
    Ambient,
    Diffuse,
    FogStartEnd,
    Sky,
    Cloud,
    Vertex,
}

fn key_of(token: &str) -> Option<Key> {
    Some(match token {
        "TOWN" => Key::Town,
        "C_useEnvir" => Key::UseEnvir,
        "C_useRealData" => Key::UseRealData,
        "C_id" => Key::Id,
        "C_weather" => Key::Weather,
        "C_name" => Key::Name,
        "C_ambient" => Key::Ambient,
        "C_diffuse" => Key::Diffuse,
        "C_fogstartend" => Key::FogStartEnd,
        "C_sky" => Key::Sky,
        "C_cloud" => Key::Cloud,
        "VERTEX" => Key::Vertex,
        _ => return None,
    })
}

/// Parses a `.wld.cnt` (Plan/30 §9.5). A block without its `Continent END` at the end of the file is dropped
/// with `WorldTextTruncated`; a key cut off by the end of the file is dropped with its block.
pub fn parse_cnt(text: &str, file: &str, ctx: &ScriptCtx, diags: &mut Diagnostics) -> CntFile {
    let mut out = CntFile::default();
    let mut s = open_script(text, file, ctx, diags);
    // The block being filled: created by `Continent BEGIN` or, leniently, by the first key outside a block.
    let mut cur: Option<ContinentDef> = None;
    loop {
        s.get_token();
        if s.finished() {
            break;
        }
        if s.is("WORLD") {
            s.get_token();
            if s.finished() {
                report_truncated(&mut s, "the WORLD line");
                break;
            }
            out.world = Some(s.token().to_owned());
        } else if s.is("Continent") {
            s.get_token();
            if s.is("BEGIN") {
                cur = Some(ContinentDef::default());
            } else if s.is("END")
                && let Some(block) = cur.take()
            {
                out.continents.push(block);
            }
        } else if let Some(key) = key_of(s.token()) {
            read_key(&mut s, key, cur.get_or_insert_with(ContinentDef::default));
            if s.finished() {
                report_truncated(&mut s, "a continent key");
                cur = None; // reported here; the block is dropped with it
                break;
            }
        }
    }
    if cur.is_some() {
        report_truncated(&mut s, "a continent block without its END");
    }
    out
}

/// Reads the arguments of one block key into `block`.
fn read_key(s: &mut Script<'_>, key: Key, block: &mut ContinentDef) {
    match key {
        Key::Town => block.town = s.get_number() != 0,
        Key::UseEnvir => block.use_envir = s.get_number() != 0,
        Key::UseRealData => block.use_real_data = s.get_number() != 0,
        Key::Id => block.id = s.get_number(),
        Key::Weather => block.weather = Some(s.get_number()),
        Key::Name => {
            s.get_token();
            block.name = s.token().to_owned();
        }
        Key::Ambient => block.ambient = Some(vec3(s)),
        Key::Diffuse => block.diffuse = Some(vec3(s)),
        Key::FogStartEnd => block.fog = Some((s.get_float(), s.get_float())),
        Key::Sky => block.sky = Some(flag_and_name(s)),
        Key::Cloud => block.cloud = Some(flag_and_name(s)),
        Key::Vertex => block.vertices.push(vec3(s)),
    }
}
