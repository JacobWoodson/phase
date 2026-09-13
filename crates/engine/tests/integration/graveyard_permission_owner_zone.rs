//! CR 109.4 + CR 108.4a + CR 109.5: "your graveyard" is owner-scoped.
//!
//! **The rule.** CR 109.4: "Only objects on the stack or on the battlefield have
//! a controller. Objects that are neither on the stack nor on the battlefield
//! aren't controlled by any player." None of its six exceptions covers a card in
//! a graveyard. CR 108.4a then says anything asking for such a card's controller
//! "use its owner instead", and CR 109.5 spells out the same for the word itself:
//! "you"/"your" refer to "its owner (if it has no controller)".
//!
//! So a `your graveyard` permission filter must resolve against the card's
//! OWNER. The engine already knows this — `zones.rs` cites exactly these rules
//! when it resets a departing permanent's `controller` to the owner fallback, and
//! `filter::is_owner_scoped_zone` is `Hand | Library | Graveyard`. The four
//! graveyard-permission consumers in `game::casting` were simply never routed
//! through `filter::matches_target_filter_for_zone`, the documented single
//! authority for that substitution.
//!
//! **What actually diverged.** Not the live `controller` field: `zones.rs`'s
//! `reset_for_battlefield_exit` forces `base_controller = Some(owner)` and the
//! destination-keyed reset then writes the owner back, so a graveyard card's live
//! controller is already correct. The divergence is the **LKI cache**.
//! `filter::effective_controller` reads `state.lki_cache[id].controller` for any
//! object off the battlefield/stack under `ControllerLookup::LiveOrLki`, and the
//! LKI snapshot is taken BEFORE that reset — so for a permanent that died under
//! an opponent's control it holds the THIEF.
//!
//! `matches_target_filter_in_owner_zone` passes `LiveOnly` instead, which is what
//! cures it.
//!
//! **Blast radius: the printed graveyard-permission class**, which is why this
//! suite deliberately uses a printed Ramunap-Excavator-shaped source rather than
//! any one card's parsed text. Muldrotha, Karador, Lurrus and Ramunap Excavator
//! all silently refused a card their controller owned, if an opponent happened to
//! control it when it died, for the remainder of that step (CR 400.7 —
//! `turns.rs` clears the LKI cache on step transition).
//!
//! The failure direction is **mis-exclusion only**: the owner's own card is
//! refused. No card is wrongly admitted by the pre-fix behaviour, so nothing
//! depended on it.

use engine::game::casting::{
    graveyard_lands_playable_by_permission, spell_objects_available_to_cast,
};
use engine::game::scenario::GameScenario;
use engine::types::ability::{CardPlayMode, ControllerRef, TargetFilter, TypeFilter, TypedFilter};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::statics::{CastFrequency, StaticMode};
use engine::types::zones::Zone;
use engine::types::{CardId, ObjectId, StaticDefinition};

/// A printed "you may play/cast <filter> from your graveyard" permission, of the
/// shape Ramunap Excavator / Muldrotha / Karador print.
fn printed_graveyard_permission(
    play_mode: CardPlayMode,
    types: Vec<TypeFilter>,
) -> StaticDefinition {
    StaticDefinition::new(StaticMode::GraveyardCastPermission {
        frequency: CastFrequency::Unlimited,
        play_mode,
        graveyard_destination_replacement: None,
        extra_cost: None,
        enters_with_counter: None,
    })
    .affected(TargetFilter::Typed(TypedFilter {
        type_filters: types,
        // THE AXIS UNDER TEST. "your graveyard" — which for a card with no
        // controller (CR 109.4) must resolve to its OWNER.
        controller: Some(ControllerRef::You),
        ..Default::default()
    }))
}

/// Stage `card_id` as a permanent OWNED by `owner`, gain control of it with
/// `thief`, then let it die into its owner's graveyard.
///
/// Goes through the production zone-change path (`zones::move_to_zone`) rather
/// than hand-setting fields, so the resulting state is one the engine actually
/// produces: live `controller` reset to the owner, LKI holding the thief.
fn steal_then_bury(
    runner: &mut engine::game::scenario::GameRunner,
    object_id: ObjectId,
    thief: PlayerId,
) {
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(
        runner.state_mut(),
        object_id,
        Zone::Battlefield,
        &mut events,
    );
    {
        // CR 613.1b layer 2: an opponent gains control of the permanent.
        let obj = runner
            .state_mut()
            .objects
            .get_mut(&object_id)
            .expect("staged object");
        obj.base_controller = Some(thief);
        obj.controller = thief;
    }
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(runner.state_mut(), object_id, Zone::Graveyard, &mut events);
}

/// Assert the fixture really is in the divergent state this suite exists to
/// cover, BEFORE asserting anything about permissions.
///
/// Without this the rows would pass vacuously the moment anything advanced a
/// step (CR 400.7 clears the LKI cache) or the control change failed to take —
/// and the assertion under test would be measuring nothing.
fn assert_lki_diverges(
    state: &engine::types::game_state::GameState,
    object_id: ObjectId,
    owner: PlayerId,
    thief: PlayerId,
) {
    let obj = &state.objects[&object_id];
    assert_eq!(
        obj.zone,
        Zone::Graveyard,
        "staging: the card must be in a graveyard"
    );
    assert_eq!(obj.owner, owner, "staging: owner");
    assert_eq!(
        obj.controller, owner,
        "staging: CR 109.4 — zones.rs resets the LIVE controller to the owner on exit, so the \
         live field is NOT the divergence this suite covers"
    );
    assert_eq!(
        state.lki_cache.get(&object_id).map(|lki| lki.controller),
        Some(thief),
        "staging: the LKI snapshot must still hold the THIEF — that is the divergence under test"
    );
}

/// THE discriminating row for the land path. Pre-fix this returned `[]`.
#[test]
fn a_land_that_died_under_an_opponents_control_is_still_its_owners_to_play() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let land = scenario.add_land_to_hand(PlayerId(0), "Forest").id();
    let mut runner = scenario.build();

    steal_then_bury(&mut runner, land, PlayerId(1));

    let source = engine::game::zones::create_object(
        runner.state_mut(),
        CardId(5150),
        PlayerId(0),
        "Ramunap Excavator".to_string(),
        Zone::Battlefield,
    );
    runner
        .state_mut()
        .objects
        .get_mut(&source)
        .expect("permission source")
        .static_definitions
        .push(printed_graveyard_permission(
            CardPlayMode::Play,
            vec![TypeFilter::Land],
        ));

    assert_lki_diverges(runner.state(), land, PlayerId(0), PlayerId(1));

    let playable = graveyard_lands_playable_by_permission(runner.state(), PlayerId(0));
    assert!(
        playable.iter().any(|(object_id, _)| *object_id == land),
        "CR 109.4 + CR 108.4a: a card in a graveyard has no controller, so \"your \
         graveyard\" resolves to its OWNER — the land its owner is querying for must be \
         offered even though an opponent controlled it when it died, got {playable:?}"
    );
}

/// The CAST sibling of the row above, through the public legal-cast surface.
///
/// The land and spell halves are served by DIFFERENT consumers
/// (`graveyard_lands_playable_by_permission` vs
/// `graveyard_object_castable_by_permission_sources`), so one row cannot cover
/// both — a fix applied to only one would leave them disagreeing.
#[test]
fn a_spell_that_died_under_an_opponents_control_is_still_its_owners_to_cast() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let creature = scenario
        .add_creature_to_hand(PlayerId(0), "Gravedigger", 2, 2)
        .id();
    let mut runner = scenario.build();

    steal_then_bury(&mut runner, creature, PlayerId(1));

    let source = engine::game::zones::create_object(
        runner.state_mut(),
        CardId(5151),
        PlayerId(0),
        "Muldrotha, the Gravetide".to_string(),
        Zone::Battlefield,
    );
    runner
        .state_mut()
        .objects
        .get_mut(&source)
        .expect("permission source")
        .static_definitions
        .push(printed_graveyard_permission(
            CardPlayMode::Cast,
            vec![TypeFilter::Creature],
        ));

    assert_lki_diverges(runner.state(), creature, PlayerId(0), PlayerId(1));

    let castable = spell_objects_available_to_cast(runner.state(), PlayerId(0));
    assert!(
        castable.contains(&creature),
        "CR 109.4 + CR 108.4a: the owner's own creature card must be offered from their \
         graveyard even though an opponent controlled it when it died, got {castable:?}"
    );
}

/// GUARD: the substitution must not widen the permission to cards the querying
/// player does not own.
///
/// The fix replaces a controller comparison with an owner comparison — it must
/// not degrade into "match anything in any graveyard". This row fails if the
/// owner axis is dropped rather than substituted.
#[test]
fn the_owner_substitution_does_not_widen_to_another_players_card() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let own_land = scenario.add_land_to_hand(PlayerId(0), "Forest").id();
    let opponent_land = scenario.add_land_to_hand(PlayerId(1), "Island").id();
    let mut runner = scenario.build();

    // Both die into their OWN graveyards; only P0's was ever stolen.
    steal_then_bury(&mut runner, own_land, PlayerId(1));
    let mut events = Vec::new();
    engine::game::zones::move_to_zone(
        runner.state_mut(),
        opponent_land,
        Zone::Graveyard,
        &mut events,
    );

    let source = engine::game::zones::create_object(
        runner.state_mut(),
        CardId(5152),
        PlayerId(0),
        "Ramunap Excavator".to_string(),
        Zone::Battlefield,
    );
    runner
        .state_mut()
        .objects
        .get_mut(&source)
        .expect("permission source")
        .static_definitions
        .push(printed_graveyard_permission(
            CardPlayMode::Play,
            vec![TypeFilter::Land],
        ));

    let playable = graveyard_lands_playable_by_permission(runner.state(), PlayerId(0));

    // REACH-GUARD: the permission is live, so the negative below cannot pass
    // because nothing was granted at all.
    assert!(
        playable.iter().any(|(object_id, _)| *object_id == own_land),
        "reach-guard: the owner's own land must be offered, got {playable:?}"
    );
    assert!(
        !playable
            .iter()
            .any(|(object_id, _)| *object_id == opponent_land),
        "CR 108.4a: the owner substitution must not widen the permission to a card owned \
         by another player, got {playable:?}"
    );
}
