//! `resdata.inc`: the window layouts (`CResManager::Load`, Plan/14 §6.19, ref/22 §7;
//! src:_Common/ResManager.cpp:76-154).
//!
//! ```text
//! file    := { applet }*
//! applet  := APP_NAME(no define substitution) TEXTURE TILE CX CY STYLE D3DFMT
//!            '{' TITLE '}' '{' HELP_KEY '}' '{' { control }* '}'
//! control := WTYPE CTRL_NAME(no substitution) TEXTURE TILE LEFT TOP RIGHT BOTTOM STYLE VISIBLE GROUP DISABLED
//!            TABSTOP '{' CAPTION '}' '{' TOOLTIP '}'
//! ```
//!
//! 365 applets with 2,809 controls in the reference data. `APP_`/`WIDC_` names resolve through the define
//! table (`GetDefineNum`; unknown gives -1 and a Warning, 4 controls are expected to be unknown).
//! `visible`, `disabled` and `tab_stop` are parsed but ignored by the original client; they are kept here
//! for fidelity and dropped from the bundle.

use serde::{Deserialize, Serialize};

/// One window layout (`WNDAPPLET`), in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applet {
    /// `APP_*` name as written.
    pub name: String,
    /// `GetDefineNum(name)`, -1 when the define is unknown.
    pub id: i32,
    /// Frame skin: a tile-set name (`tile`) or a full background image (a bare file name under `Theme/`).
    pub texture: String,
    /// `bTile`: 12-tile skin versus single image.
    pub tile: bool,
    /// Initial width including the frame.
    pub w: i32,
    /// Initial height including the frame.
    pub h: i32,
    /// `WBS_*` bits.
    pub style: u32,
    /// `D3DFORMAT` of the baked texture (26 or 21; irrelevant for the web).
    pub d3d_format: u32,
    /// The window title (IDS key resolved, may be empty).
    pub title: String,
    /// The help key: non-empty adds the `?` button (`strToolTip`).
    pub help_key: String,
    /// The controls in file order (paint and hit-test order).
    pub controls: Vec<Control>,
}

/// One control of an applet (`WNDCTRL`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    /// `WTYPE_*` value (button 1, static, custom, edit, text, list box, ...: `WndStyle.h`).
    pub wtype: u32,
    /// `WIDC_*` name as written (some are IDS keys by mistake).
    pub name: String,
    /// `GetDefineNum(name)`, -1 when unknown.
    pub id: i32,
    /// Skin or sprite-strip texture (a bare file name under `Theme/`), may be empty.
    pub texture: String,
    /// `bTile`.
    pub tile: bool,
    /// `left, top, right, bottom` relative to the parent client area (may be negative).
    pub rect: [i32; 4],
    /// Control style bits.
    pub style: u32,
    /// Parsed, ignored by the client.
    pub visible: bool,
    /// Button only: starts a radio group.
    pub group: bool,
    /// Parsed, ignored by the client.
    pub disabled: bool,
    /// Parsed, ignored by the client.
    pub tab_stop: bool,
    /// Caption (buttons, statics, texts).
    pub caption: String,
    /// Hover text.
    pub tooltip: String,
}

/// What `parse_resdata` reports besides the applets.
#[cfg(feature = "load")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResdataStats {
    /// Applets whose control block is `{ }`.
    pub empty_blocks: u32,
    /// Applet or control names that are not defines (their id is -1).
    pub unknown_names: u32,
}

#[cfg(feature = "load")]
pub(crate) use parse::parse_resdata;

#[cfg(feature = "load")]
mod parse {
    use super::{Applet, Control, ResdataStats};
    use flyff_script::{DiagCode, Diagnostics, ScannerOptions, Script, ScriptCtx, Severity};
    use std::sync::Arc;

    /// `GetLangApplet` / `GetLangCtrl`: `{`, one token, `}`.
    fn lang(sc: &mut Script<'_>) -> String {
        sc.get_token(); // {
        sc.get_token();
        let s = sc.token().to_owned();
        sc.get_token(); // }
        s
    }

    /// `GetDefineNum(name)`: the define value, -1 (with a Warning) when it is not defined.
    fn define_num(sc: &mut Script<'_>, name: &str, stats: &mut ResdataStats) -> i32 {
        match sc.ctx().defines.get(name) {
            Some(v) => v,
            None => {
                stats.unknown_names = stats.unknown_names.saturating_add(1);
                sc.report(
                    Severity::Warning,
                    DiagCode::UnknownIdentifier,
                    format!("resdata: {name} is not a define; its id is -1"),
                );
                -1
            }
        }
    }

    /// `CResManager::Load` over decoded `text`. A truncated file ends the current applet; an unterminated
    /// control block stops at the end of input (the original would loop on its last token).
    pub(crate) fn parse_resdata(
        text: &str,
        file: &Arc<str>,
        sctx: &ScriptCtx,
        diags: &mut Diagnostics,
    ) -> (Vec<Applet>, ResdataStats) {
        let mut out = Vec::new();
        let mut stats = ResdataStats::default();
        let mut sc = Script::new(
            text,
            Arc::clone(file),
            ScannerOptions::default(),
            sctx,
            diags,
        );
        sc.get_token_nodef();
        while !sc.finished() {
            let name = sc.token().to_owned();
            let id = define_num(&mut sc, &name, &mut stats);
            sc.get_token();
            let texture = sc.token().to_owned();
            let tile = sc.get_number() != 0;
            let w = sc.get_number_i32();
            let h = sc.get_number_i32();
            let style = sc.get_number();
            let d3d_format = sc.get_number();
            let title = lang(&mut sc);
            let help_key = lang(&mut sc);
            sc.get_token(); // {
            let mut controls = Vec::new();
            let mut wtype = sc.get_number();
            while !sc.token().starts_with('}') && !sc.finished() {
                sc.get_token_nodef();
                let cname = sc.token().to_owned();
                let cid = define_num(&mut sc, &cname, &mut stats);
                sc.get_token();
                let ctexture = sc.token().to_owned();
                let ctile = sc.get_number() != 0;
                let rect = [
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                    sc.get_number_i32(),
                ];
                let cstyle = sc.get_number();
                let visible = sc.get_number() != 0;
                let group = sc.get_number() != 0;
                let disabled = sc.get_number() != 0;
                let tab_stop = sc.get_number() != 0;
                let caption = lang(&mut sc);
                let tooltip = lang(&mut sc);
                controls.push(Control {
                    wtype,
                    name: cname,
                    id: cid,
                    texture: ctexture,
                    tile: ctile,
                    rect,
                    style: cstyle,
                    visible,
                    group,
                    disabled,
                    tab_stop,
                    caption,
                    tooltip,
                });
                wtype = sc.get_number();
            }
            if controls.is_empty() {
                stats.empty_blocks = stats.empty_blocks.saturating_add(1);
            }
            out.push(Applet {
                name,
                id,
                texture,
                tile,
                w,
                h,
                style,
                d3d_format,
                title,
                help_key,
                controls,
            });
            sc.get_token_nodef();
        }
        (out, stats)
    }
}
