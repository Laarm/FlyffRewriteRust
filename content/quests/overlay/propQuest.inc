// propQuest.inc of the SYNTHETIC QUEST SET (task quest-proof, WP-NPC-17; Plan/20 section 13). A drop-in `overrides/`
// file valid ON TOP OF THE REAL Resource, whose propQuest*.inc files are empty (PD-01). Mount it with
// FLYFF__PATHS__OVERRIDES_DIR=<repo>/content/quests/overlay, or copy the three files into <home>/overrides/; never
// shipped active in the dist. Pure ASCII, read as UTF-8 without BOM. See content/quests/README.md.
//
// 13 quests: 9001-9011, 51 (QUEST_VALENTINE) and 500 (the quest of the real text item II_SYS_SYS_QUE_BOOK).
// Givers: the Flaris shop keepers MaFl_Losha and MaFl_Dior. Every name below exists in the real headers.

// ---- 9001: kill two Aibatts (Losha -> Losha) ----
9001
{
	SetTitle( IDS_PROPQUEST_INC_000001 );
	SetCharacter( "MaFl_Losha" );
	SetBeginCondLevel( 1, 150 );
	SetEndCondKillNPC( 0, MI_AIBATT1, 2 );
	SetEndRewardItem( -1, 0, -1, II_GEN_FOO_INS_BISCUIT, 3 );
	SetEndRewardGold( 1000, 1000 );
	SetEndRewardExp( 500, 500 );
	SetDialog( QSAY_BEGIN1, IDS_PROPQUEST_INC_000002 );
	SetDialog( QSAY_BEGIN_YES, IDS_PROPQUEST_INC_000003 );
	SetDialog( QSAY_BEGIN_NO, IDS_PROPQUEST_INC_000004 );
	SetDialog( QSAY_END_COMPLETE1, IDS_PROPQUEST_INC_000005 );
	SetDialog( QSAY_END_FAILURE1, IDS_PROPQUEST_INC_000006 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000007 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000008 );
	}
}

// ---- 9002: vision stones dropped by Aibatts (Losha -> Losha); 0xB2D05E00 is 3 000 000 000 ----
9002
{
	SetTitle( IDS_PROPQUEST_INC_000009 );
	SetCharacter( "MaFl_Losha" );
	QuestItem( MI_AIBATT1, II_SYS_SYS_QUE_VISIONSTONE, 0xB2D05E00, 2 );
	SetEndCondItem( -1, 0, -1, II_SYS_SYS_QUE_VISIONSTONE, 3 );
	SetEndRemoveItem( 0, II_SYS_SYS_QUE_VISIONSTONE, 3 );
	SetEndRewardGold( 200, 400 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000010 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000011 );
	}
}

// ---- 9003: a party quest, the kill counts for every party member in range (Losha -> Losha) ----
9003
{
	SetTitle( IDS_PROPQUEST_INC_000012 );
	SetCharacter( "MaFl_Losha" );
	SetBeginCondParty( 2, 1, 2, -1 );
	SetEndCondKillNPC( 0, MI_AIBATT1, 3 );
	SetEndCondParty( 2, 1, 2, 0 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000013 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000014 );
	}
}

// ---- 9004: a timed errand that cannot be given up: two minutes (Dior -> Losha) ----
// The kill condition keeps the quest from being completable, so the timer runs (can_end freezes it).
9004
{
	SetTitle( IDS_PROPQUEST_INC_000015 );
	SetCharacter( "MaFl_Dior" );
	SetEndCondLimitTime( 2 );
	SetEndCondKillNPC( 0, MI_AIBATT1, 1 );
	SetEndCondDialog( "MaFl_Dior", "Introduction" );
	SetEndCondCharacter( "MaFl_Losha" );
	SetRemove( 0 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000016 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000017 );
	}
}

// ---- 9005: states 1..3, a patrol zone and a kill; SetEndCondState( 0 ) means the last defined state, 3 (Dior -> Dior) ----
// The zone is a few metres from the Flaris start (6968, 3328), not on it.
9005
{
	SetTitle( IDS_PROPQUEST_INC_000018 );
	SetCharacter( "MaFl_Dior" );
	SetEndCondState( 0 );
	SetEndCondPatrolZone( WI_WORLD_MADRIGAL, 6980, 3300, 7000, 3320 );
	SetEndCondKillNPC( 0, MI_PUKEPUKE1, 1 );
	state 1
	{
		SetDesc( IDS_PROPQUEST_INC_000019 );
	}
	state 2
	{
		SetDesc( IDS_PROPQUEST_INC_000020 );
	}
	state 3
	{
		SetDesc( IDS_PROPQUEST_INC_000021 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000022 );
	}
}

// ---- 9006: shown red ("next level") below level 5, offered from level 5 to 10 (Dior -> Dior) ----
9006
{
	SetTitle( IDS_PROPQUEST_INC_000023 );
	SetCharacter( "MaFl_Dior" );
	SetBeginCondLevel( 5, 10 );
}

// ---- 9007: a repeatable quest: 100 penya for a bottle of milk, never in the completed list (Losha -> Losha) ----
9007
{
	SetTitle( IDS_PROPQUEST_INC_000024 );
	SetCharacter( "MaFl_Losha" );
	SetRepeat( 1 );
	SetEndCondGold( 100 );
	SetEndRemoveGold( 100 );
	SetEndRewardItem( -1, 0, -1, II_GEN_FOO_INS_MILK, 1 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000025 );
	}
}

// ---- 9008: a follow-up of 9001; completing it removes 9007 (Losha -> Losha) ----
9008
{
	SetTitle( IDS_PROPQUEST_INC_000026 );
	SetCharacter( "MaFl_Losha" );
	SetBeginCondPreviousQuest( 1, 9001 );
	SetEndCondCompleteQuest( 1, 9001 );
	SetEndRemoveQuest( 9007 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000027 );
	}
}

// ---- 9009: starts with items and gold, can be given up (Dior -> Dior) ----
9009
{
	SetTitle( IDS_PROPQUEST_INC_000028 );
	SetCharacter( "MaFl_Dior" );
	SetBeginSetAddItem( 0, II_GEN_FOO_INS_MILK, 2 );
	SetBeginSetAddGold( 50 );
	SetRemove( 1 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000029 );
	}
}

// ---- 9010: the begin conditions: job, sex, an exclusive quest, 1 to 4 vision stones (Dior -> Dior) ----
9010
{
	SetTitle( IDS_PROPQUEST_INC_000030 );
	SetCharacter( "MaFl_Dior" );
	SetBeginCondJob( JOB_VAGRANT, JOB_KNIGHT );
	SetBeginCondSex( -1 );
	SetBeginCondExclusiveQuest( 9007 );
	SetBeginCondItem( -1, 0, -1, II_SYS_SYS_QUE_VISIONSTONE, 1 );
	SetBeginCondNotItem( -1, 0, -1, II_SYS_SYS_QUE_VISIONSTONE, 5 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000031 );
	}
}

// ---- 500: the quest of the real text item II_SYS_SYS_QUE_BOOK, started by using the book; no giver (-> Losha) ----
500
{
	SetTitle( IDS_PROPQUEST_INC_000032 );
	SetEndCondCharacter( "MaFl_Losha" );
	SetEndRewardItem( -1, 0, -1, II_GEN_FOO_INS_BISCUIT, 1 );
	SetEndRewardGold( 100, 100 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000033 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000034 );
	}
}

// ---- QUEST_VALENTINE (51): an Arcard stone from Aibatts, scaled by the level difference (Losha -> Losha) ----
QUEST_VALENTINE
{
	SetTitle( IDS_PROPQUEST_INC_000035 );
	SetCharacter( "MaFl_Losha" );
	QuestItem( MI_AIBATT1, II_SYS_SYS_QUE_ARCARDSTONE, 1500000000, 1 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000036 );
	}
}

// ---- 9011: the catalogue: every directive of the grammar that the quests above do not use (ref/16 section 4.4) ----
// Linked to Losha and Dior but never offered: its begin conditions cannot all be met (campus points 50, tutorial
// state 4, a guild of 40 members, a disguise-free female with a pet of level 2, ...). It exists to be parsed, projected
// into the client bundle and validated. The goal tails (x, z, GOAL_TEXT_ID_nnnn) are part of SetEndCondItem, KillNPC,
// PatrolZone, Character and Dialog in the V15 files; the counter below runs 1..6.
9011
{
	SetTitle( IDS_PROPQUEST_INC_000037 );
	SetNPCName( IDS_PROPQUEST_INC_000038 );
	SetMultiCharacter( "MaFl_Losha", 0, "MaFl_Dior", 0 );
	SetBeginCondSex( 1 );
	SetBeginCondSkillLvl( SI_VAG_ONE_CLEANHIT, 3 );
	SetBeginCondPKValue( 5 );
	SetBeginCondNotItem( -1, 1, II_WEA_SWO_WOODEN, II_GEN_FOO_INS_BISCUIT, 20 );
	SetBeginCondLevel( 20, 60 );
	SetBeginCondParty( 2, 1, 3, 1 );
	SetBeginCondGuild( 2, 1, 40, 0 );
	SetBeginCondJob( JOB_MERCENARY, JOB_KNIGHT );
	SetBeginCondPreviousQuest( 0, 9001, 9008 );
	SetBeginCondExclusiveQuest( 9009, 9010 );
	SetBeginCondItem( 0, 0, 1, II_SYS_SYS_QUE_ARCARDSTONE, 2 );
	SetBeginCondDisguise( -1 );
	SetBeginSetDisguise( MI_PUKEPUKE1 );
	SetBeginSetAddGold( 700 );
	SetBeginSetAddItem( 1, II_GEN_FOO_INS_BISCUIT, 4 );
	SetBeginCondPetExp( 40 );
	SetBeginCondPetLevel( 2 );
	SetBeginCondTutorialState( 4 );
	SetBeginCondTSP( 50 );
	SetEndCondParty( 2, 0, 2, 2 );
	SetEndCondGuild( 1, 0, 0, 0 );
	SetEndCondState( 2 );
	SetEndCondCompleteQuest( 2, 9001, 9008 );
	SetEndCondSkillLvl( SI_VAG_ONE_CLEANHIT, 4 );
	SetEndCondLevel( 25, 80 );
	SetEndCondExpPercent( 100, 9999 );
	SetEndCondGold( 1500 );
	SetEndCondOneItem( -1, 0, 2, II_WEA_SWO_WOODEN, 1 );
	SetEndCondLimitTime( 90 );
	SetEndCondItem( 1, 0, -1, II_GEN_FOO_INS_MILK, 7, 6985.0, 3305.0, GOAL_TEXT_ID_0001 );
	SetEndCondKillNPC( 0, MI_PUKEPUKE1, 4, 6990.0, 3310.0, GOAL_TEXT_ID_0002 );
	SetEndCondKillNPC( 1, MI_AIBATT1, 2 );
	SetEndCondPatrolZone( WI_WORLD_MADRIGAL, 7010, 3350, 7000, 3340, PATROL_DESTINATION_ID_0003, 7005.0, 3345.0, GOAL_TEXT_ID_0003 );
	SetEndCondCharacter( "MaFl_Losha", 6968.0, 3328.0, GOAL_TEXT_ID_0004 );
	SetEndCondMultiCharacter( "MaFl_Losha", II_GEN_FOO_INS_MILK, "MaFl_Dior", 0 );
	SetEndCondDialog( "MaFl_Dior", "Farewell", 6970.0, 3330.0, GOAL_TEXT_ID_0005 );
	SetEndCondPetLevel( 5 );
	SetEndCondPetExp( 60 );
	SetEndCondDisguise( MI_PUKEPUKE1 );
	SetEndCondTSP( 70 );
	SetParam( 0, 11 );
	SetParam( 3, 44 );
	SetDlgRewardItem( 1, II_GEN_FOO_INS_BISCUIT, 3 );
	SetEndRewardItem( 0, 0, 1, II_GEN_FOO_INS_MILK, 2, 1 );
	SetEndRewardItemWithAbilityOption( 1, 0, 2, II_WEA_SWO_WOODEN, 1, 5, 1 );
	SetEndRewardGold( 100, 200 );
	SetEndRewardExp( 1000, 2000 );
	SetEndRewardSkillPoint( 3 );
	SetEndRewardPKValue( 1, 5 );
	SetEndRewardTeleport( WI_WORLD_MADRIGAL, 6968.0, 100.0, 3328.8 );
	SetEndRewardPetLevelup;
	SetEndRewardHide( 1 );
	SetEndRewardTSP( 9 );
	SetEndRemoveQuest( 9001, 9008 );
	SetEndRemoveItem( 0, II_GEN_FOO_INS_BISCUIT, 5 );
	SetEndRemoveGold( 321 );
	SetEndRemoveTSP( 13 );
	SetRepeat( 1 );
	SetRemove( 0 );
	QuestItem( MI_PUKEPUKE1, II_GEN_FOO_INS_MILK, 500, 1 );
	SetDialog( QSAY_EXTRA01, IDS_PROPQUEST_INC_000043 );
	SetPatrolZoneName( IDS_PROPQUEST_INC_000044 );
	SetHeadQuest( 1996 );
	SetQuestType( 2 );
	state 0
	{
		SetDesc( IDS_PROPQUEST_INC_000039 );
		SetCond( IDS_PROPQUEST_INC_000040 );
		SetStatus( IDS_PROPQUEST_INC_000041 );
		QuestItem( MI_AIBATT1, II_GEN_FOO_INS_MILK, 500, 1 );
	}
	state 1
	{
		SetDesc( IDS_PROPQUEST_INC_000045 );
	}
	state 14
	{
		SetDesc( IDS_PROPQUEST_INC_000042 );
	}
}
