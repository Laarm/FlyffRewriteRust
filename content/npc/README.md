# content/npc

Transpiled NPC dialog scripts (Lua 5.3), generated from the original `NpcScript.cpp` by
`tools/npcscript-transpile` and committed (Plan/20 §5, D2). Shipped as `scripts/npc/` next to the executable
(`flyff` restores a missing folder from the copy embedded in the binary, `flyff check` compares the folder with
`MANIFEST.txt`).

* `<npc>.lua`: one file per script (271), lower-case stem = `NpcDef.dialog_key`. `return { format = 1, script,
  defines, [<WorldDialog.txt key line>] = function(d) ... end }`. Calls are `d:Helper(...)` with the helper names
  of the ScriptLib table; numeric arguments of `Say/Speak/AddKey/...` are `WorldDialog.txt` line ids; `C.NAME` is a
  define constant (listed in `defines`). The functions the original server never calls sit in a trailing
  `--[==[ UNREACHABLE ... ]==]` comment.
* `MANIFEST.txt`: `<sha256>  <file>` per `.lua` file.
* `_transpile.toml`: format and tool version, sha256 of the inputs, the measured counts (Plan/20 §5.2).

Do not edit the generated files by hand: they are reproduced byte for byte by
`cargo xtask npc-transpile` (needs `FLYFF_SOURCE` and `FLYFF_RESOURCE`; `--check` only compares). Server
operators override single scripts through `paths.npc_scripts_dir`, not here.
