// propGuildQuest.inc of the SYNTHETIC QUEST SET (task quest-proof, WP-NPC-17): one guild quest with every key that
// CProject::LoadPropGuildQuest reads (Title, Level, Wormon, World, Position, Region, State n { desc }).
//
// QUEST_WARMON_LV1 (1) is the quest that the two real Darkon guild scripts (mada_eshylop, mada_cloneeshylop) test with
// IsGuildQuest / GetGuildQuestState; the Wormon boss is the first ClockWorks. MonHuntStart is a deferred stub (F-253),
// so the quest is set by the GM command: /sgq <guild name> 1 0, and /sgq <guild name> 1 14 for the finished state.
//
// The Region is NOT inert: the server installs it as the guild-quest arena (SkillService.guild_quest_regions); inside
// it the return scroll and Blinkpool are refused, on (int)x, (int)z of any world. It is written as the original
// expects, Region = x1, y1, x2, y2 with y1 > y2 (the rectangle CRect(x1, y2, x2, y1) = x 3700..3900, z 4350..4550), far
// from Flaris (6968, 3328), the Madrigal start and the small coordinates (0..1000) of the test worlds.
QUEST_WARMON_LV1
{
	Title = "Wormon Level 1"
	Level = 70
	Wormon = MI_CLOCKWORK1
	World = WI_WORLD_MADRIGAL
	Position = 3800.0, 100.0, 4450.0
	Region = 3700, 4550, 3900, 4350
	State 0 { desc = "Defeat the Wormon in the arena within an hour." }
	State 14 { desc = "The Wormon was defeated." }
}
