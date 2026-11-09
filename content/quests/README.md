# The synthetic quest set

The shipped game has **no quests**: the five `propQuest*.inc` files of the original Resource are empty (PD-01, UD-03),
so the quest UI stays hidden (PL-148) and every NPC shows its key buttons and its `#noQuest` greeting. This folder
holds a small quest set that proves the quest engine without the original quest data (task `quest-proof`, WP-NPC-17,
Plan/20 §13). It is used by the in-crate proof tests, by the bot scenarios, by the demo D-M7 step 9 and by the
Playwright flows of the quest windows.

`overlay/` is a drop-in `overrides/` directory that is valid **on top of the real Resource**. It is not indexed by the
virtual file system unless you mount it (this README lives outside it on purpose) and is never active in the
packaged game.

| file | content |
|---|---|
| `overlay/propQuest.inc` | 13 quests: 9001-9011, 51 (`QUEST_VALENTINE`) and 500 (the quest of the real text book) |
| `overlay/propQuest.txt.txt` | their texts, `IDS_PROPQUEST_INC_000001..` (the four other `propQuest*` files stay empty) |
| `overlay/propGuildQuest.inc` | one guild quest `QUEST_WARMON_LV1` (1) with every key `LoadPropGuildQuest` reads |

All three files are pure ASCII (UTF-8 without BOM), so they are reviewable in a diff. Every name they use exists in the
real headers (`definequest.h`, `defineItem.h`, `defineObj.h`, `defineWorld.h`, `defineSkill.h`, `defineJob.h`).

## Mounting it

* On a server: `FLYFF__PATHS__OVERRIDES_DIR=<repo>/content/quests/overlay flyff serve ...`, or copy the three files into
  `<home>/overrides/` **before** the start (the index of the overrides is built at the start; a file created later is
  invisible). The start log counts 13 quests, 1 guild quest and 0 party quests (C18).
* In a test of `flyff-game`: `quest::proof_tests::overlay_tables()` loads it without the Resource through
  `flyff_data::load::steps::quests::quest_tables_from`; `res_proof_overlay_on_the_real_resource` loads it with
  `VfsConfig::with_overrides` (the server's path).
* The bots: `tests/tests/bot_quest.rs` starts a real-pack server with `paths.overrides_dir` set to this folder.
* The Playwright flows of the quest windows copy the three files plus their own addendum (`web/e2e/quest/overlay/`,
  quest ids 9020-9023 at the 0-menu NPC `MaFl_Gergantes`) into a private server home.

## The quests

Givers and end NPCs are the two Flaris shop keepers `MaFl_Losha` and `MaFl_Dior`. **Their click opens the shop, not
the dialog** (`AddMenu( MMI_DIALOG )` is commented out in `character.inc`, KQ-26): a browser reaches their dialog only
through the `npcMenuRun(MMI_DIALOG, objId)` probe of `web/src/features/npc/menuModule.ts`; the server's `ScriptDlg`
has no menu check, so the tests and the bots (`npc_open` sends the empty key = `#init`) talk to them directly. The
clickable human flow is the addendum quest at `MaFl_Gergantes`.

| id | giver -> end | what it covers |
|---|---|---|
| 9001 | Losha -> Losha | level 1-150, kill 2 `MI_AIBATT1`, rewards 3 biscuits, 1000 penya, 500 EXP; the five quest dialog lines |
| 9002 | Losha -> Losha | `QuestItem( MI_AIBATT1, II_SYS_SYS_QUE_VISIONSTONE, 3 000 000 000, 2 )`, needs 3 stones, removes 3, 200-400 penya |
| 9003 | Losha -> Losha | party quest: begins in a party of 2 or more, 3 Aibatt kills shared with every member within 64 m |
| 9004 | Dior -> Losha | timed errand: 2 minutes, kill 1 Aibatt, `SetEndCondDialog( "MaFl_Dior", "Introduction" )`, cannot be given up (`SetRemove( 0 )`) |
| 9005 | Dior -> Dior | states 1..3, `SetEndCondState( 0 )` (= state 3), a patrol zone next to the Flaris start, kill 1 `MI_PUKEPUKE1` |
| 9006 | Dior -> Dior | `SetBeginCondLevel( 5, 10 )`: shown red below level 5, offered from 5 to 10 |
| 9007 | Losha -> Losha | repeatable (`SetRepeat( 1 )`), needs and removes 100 penya, rewards a bottle of milk; never in the completed list |
| 9008 | Losha -> Losha | follow-up: begins after 9001 is completed, completing it removes 9007 (`SetEndRemoveQuest`) |
| 9009 | Dior -> Dior | start items (2 milk) and 50 penya; can be given up |
| 9010 | Dior -> Dior | begin conditions: job, sex, exclusive quest 9007, 1 to 4 vision stones |
| 500 | (the book) -> Losha | the quest of the real text item `II_SYS_SYS_QUE_BOOK` (10383, permanent): using the book starts it; completing it at Losha pays a biscuit and 100 penya |
| 51 | Losha -> Losha | `QUEST_VALENTINE`: an Arcard stone from Aibatts, the drop chance scaled by the level gap (x1, x0.8, x0.6, x0.3, x0.1) |
| 9011 | (nobody) | the catalogue: every other directive of the grammar once (rewards of every kind, every condition, `SetParam`, `SetDlgRewardItem`, goal tails, `SetHeadQuest( 1996 )`, ...); linked to both NPCs but never offered (its begin conditions cannot all be met) |

13 quests and 1 guild quest cover every directive of `LoadPropQuest` and every key of `LoadPropGuildQuest`
(`quest::proof_tests::quest_proof_overlay_covers_every_directive`).

## Walkthrough of 9001 (the bot scenario S-QUEST-01)

1. Walk to Losha in Flaris and send `ScriptDlg` with an empty key (`#init`). A fresh character gets four buttons:
   *Aibatt Hunt* (9001), *Daily Milk* (9007), *Vision Stones* (9002), *Valentine Stones* (51), sorted by level range.
2. Press *Aibatt Hunt* (`QUEST_BEGIN`, quest 9001): the offer text and the two answers *Yes* / *No*.
3. *Yes* (`QUEST_BEGIN_YES`): `SetQuest{9001, state 0}`; the list is refreshed.
4. Kill two Aibatts near the start (`SetQuest` kills `[1, 0]`, then `[2, 0]`).
5. Back at Losha: `#init` now offers the completion (*OK*, `QUEST_END_COMPLETE`); the rewards arrive in the order of
   Plan/20 §9.7 (three biscuits, 1000 penya, 500 EXP) and `SetQuest{9001, state 14}` ends the quest.

A level-1 character dies in the fields: level it first (`/lv`, `/heal`) before the fight, as the bot scenarios do.

## Guild quest and the `/sgq` line

`QUEST_WARMON_LV1` is the guild quest that the two real Darkon guild scripts (`mada_eshylop`, `mada_cloneeshylop`)
test. `MonHuntStart` is a logged stub (F-253): it answers "slot busy", so the quest is set by a GM:

```text
/sgq <guild name> 1 0     # the hunt is on: mada_eshylop offers "Yes_Enter"
/sgq <guild name> 1 14    # finished: mada_cloneeshylop offers the move to the arena
```

**The `Region` is not inert.** The server installs `Region = 3700, 4550, 3900, 4350` as the guild-quest arena
(`SkillService.guild_quest_regions`; world-agnostic on `(int)x, (int)z`): inside the rectangle x 3700..3900, z
4350..4550 the return scroll and Blinkpool are refused. It lies around Darkon and is far from Flaris (6968, 3328), the
Madrigal start and the small coordinates (0..1000) of the test worlds on purpose; never move it near them.

## Rules for editing

* Use only names that exist in the real headers (`flyff_common::defs::ALL`); quest ids 9001-9011 are free in
  `definequest.h` and outside the `SetHeadQuest` remap range 1800-1999; 9012-9099 are reserved for the quest-window
  addendum (9020-9023 and the `QUEST_KIND_*` props 6000-6003).
* No `QUEST_*` that `content/npc/*.lua` reads (45 names) except `QUEST_WARMON_LV1`.
* A `QuestItem` probability above `i32::MAX` is written in hexadecimal (`0xB2D05E00` = 3 000 000 000): the scanner
  saturates a decimal number.
