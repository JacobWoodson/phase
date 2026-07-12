//! Horde Magic PR6 — shared-life multiplayer survivor team + setup turns.
//!
//! With 2–4 survivors, the survivors form ONE shared-life team (Two-Headed-Giant
//! style): they share a single combined life total (CR 810.8/810.9a), reused via
//! `topology::has_shared_life_resources`. The Horde occupies the archenemy seat
//! (seat 0) and reuses the OneVsMany game-over path. Before the Horde's first
//! turn, the survivors take `survivor_setup_turns` (3) turns to establish a board
//! (CR 805 shared team turns).
//!
//! Every test names an assertion that flips if the corresponding PR6 wiring is
//! reverted. Negative assertions are paired with a positive reach-guard proving
//! the input reached the seam under test.

use engine::game::effects::life::apply_damage_life_loss;
use engine::game::players::{team_life_total, teammates};
use engine::game::sba::check_state_based_actions;
use engine::game::turns::start_next_turn;
use engine::game::zones::create_object;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::format::{ChallengeDeck, FormatConfig};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const HORDE: PlayerId = PlayerId(0);

/// A Horde game with `survivors` survivors (seat 0 = Horde, seats 1.. = survivors).
fn horde_game(survivors: u8) -> GameState {
    GameState::new(
        FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset()),
        survivors + 1,
        42,
    )
}

/// Create a creature on the battlefield under `controller`'s control (keeps the
/// Horde "undefeated" so a survivor loss routes to a Horde win, not a defeat).
fn add_creature(state: &mut GameState, controller: PlayerId) -> ObjectId {
    let card_id = CardId(state.next_object_id);
    let id = create_object(
        state,
        card_id,
        controller,
        "Zombie".to_string(),
        Zone::Battlefield,
    );
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.base_card_types = obj.card_types.clone();
    obj.power = Some(2);
    obj.toughness = Some(2);
    obj.base_power = Some(2);
    obj.base_toughness = Some(2);
    id
}

/// Combined survivor life: with 2 survivors the survivor-team `team_life_total`
/// equals the configured combined total (20), NOT 40 (2 × 20). The combined total
/// is distributed across the survivor seats in `GameState::new`.
///
/// Revert guard: removing the Horde life-distribution block in `GameState::new`
/// leaves every survivor at the full 20, so `team_life_total` sums to 40 and the
/// `== 20` / `!= 40` assertions flip. Reach-guard: the per-seat split (10 + 10)
/// is asserted to sum to the combined total.
#[test]
fn two_survivors_share_one_combined_life_total() {
    let state = horde_game(2);
    let survivor_a = PlayerId(1);
    let survivor_b = PlayerId(2);

    // Reach-guard: the split is even (10 each) and sums to the combined total.
    let a_life = state.players[survivor_a.0 as usize].life;
    let b_life = state.players[survivor_b.0 as usize].life;
    assert_eq!(a_life, 10, "survivor A gets half the combined 20");
    assert_eq!(b_life, 10, "survivor B gets half the combined 20");
    assert_eq!(
        a_life + b_life,
        20,
        "per-seat split sums to the combined total"
    );

    // Load-bearing: the shared team total is 20, not 40.
    assert_eq!(
        team_life_total(&state, survivor_a),
        20,
        "the survivor team shares ONE combined life total of 20"
    );
    assert_ne!(
        team_life_total(&state, survivor_a),
        40,
        "the survivors must NOT each keep a full 20 (that would sum to 40)"
    );
    // Both survivors read the same shared total.
    assert_eq!(
        team_life_total(&state, survivor_a),
        team_life_total(&state, survivor_b),
        "both survivors read the same shared team total"
    );
}

/// Three survivors (player_count 4) also share one combined 20, split evenly with
/// the remainder on the first survivor (8 + 6 + 6 = 20).
///
/// Revert guard: same as the 2-survivor case — dropping the distribution leaves
/// three 20s summing to 60.
#[test]
fn three_survivors_share_one_combined_life_total() {
    let state = horde_game(3);
    let (a, b, c) = (PlayerId(1), PlayerId(2), PlayerId(3));

    // Even split of 20 across 3 survivors: 8 (remainder) + 6 + 6.
    assert_eq!(
        state.players[a.0 as usize].life, 8,
        "remainder lands on survivor A"
    );
    assert_eq!(state.players[b.0 as usize].life, 6);
    assert_eq!(state.players[c.0 as usize].life, 6);
    assert_eq!(
        state.players[a.0 as usize].life
            + state.players[b.0 as usize].life
            + state.players[c.0 as usize].life,
        20,
        "reach-guard: the 3-way split sums to the combined total"
    );

    assert_eq!(
        team_life_total(&state, a),
        20,
        "the 3-survivor team shares ONE combined life total of 20"
    );
    assert_ne!(
        team_life_total(&state, a),
        60,
        "must not sum three full 20s"
    );
}

/// Damaging one survivor reduces the SHARED team total (survivors lose life
/// individually per the redirect rules, but every read folds through the team).
///
/// Revert guard: without the shared-life predicate broadening,
/// `shared_resource_members` would return just the damaged survivor, so
/// `team_life_total` for the OTHER survivor would be unaffected — the final
/// assertion (both drop) flips.
#[test]
fn damaging_one_survivor_reduces_shared_team_total() {
    let mut state = horde_game(2);
    let survivor_a = PlayerId(1);
    let survivor_b = PlayerId(2);
    let team_before = team_life_total(&state, survivor_a);
    assert_eq!(
        team_before, 20,
        "reach-guard: team starts at the combined 20"
    );

    let mut events = Vec::new();
    apply_damage_life_loss(&mut state, survivor_b, 4, &mut events).unwrap();

    assert_eq!(
        team_life_total(&state, survivor_a),
        team_before - 4,
        "damage to survivor B lowers the shared team total"
    );
    // Both survivors observe the reduced shared total.
    assert_eq!(
        team_life_total(&state, survivor_b),
        team_before - 4,
        "survivor B reads the same reduced shared total"
    );
    assert_eq!(
        team_life_total(&state, survivor_a),
        team_life_total(&state, survivor_b),
        "the reduction is shared, not local to the damaged survivor"
    );
}

/// Team loss: when the survivors' SHARED total reaches 0 or less, ALL survivors
/// are eliminated by the CR 704.5a / CR 810.8c life SBA and the Horde wins via the
/// archenemy game-over path — even though one survivor is still individually
/// positive. This proves the loss is on the shared total, not per-seat.
///
/// Revert guard: without the shared-life broadening, `team_life_total(survivor_a)`
/// would equal survivor A's own +5 (positive), so A would NOT be collected as a
/// loser — the "both eliminated" assertion flips.
#[test]
fn shared_total_at_zero_loses_the_whole_team_horde_wins() {
    let mut state = horde_game(2);
    let survivor_a = PlayerId(1);
    let survivor_b = PlayerId(2);
    add_creature(&mut state, HORDE); // Horde undefeated → survivor loss = Horde win.

    // One survivor positive individually, the other deep negative, so the SHARED
    // total is -1 (<= 0) while survivor A alone is +5.
    state.players[survivor_a.0 as usize].life = 5;
    state.players[survivor_b.0 as usize].life = -6;
    state.phase = Phase::PreCombatMain;
    state.active_player = HORDE;
    state.priority_player = HORDE;
    state.waiting_for = WaitingFor::Priority { player: HORDE };

    // Reach-guard: survivor A is individually positive, but the TEAM total is <= 0.
    assert!(
        state.players[survivor_a.0 as usize].life > 0,
        "reach-guard: survivor A is individually still alive on its own life"
    );
    assert!(
        team_life_total(&state, survivor_a) <= 0,
        "reach-guard: the shared team total is 0 or less"
    );

    let mut events: Vec<GameEvent> = Vec::new();
    check_state_based_actions(&mut state, &mut events);

    assert!(
        state.players[survivor_a.0 as usize].is_eliminated,
        "survivor A (individually positive) is eliminated because the TEAM lost"
    );
    assert!(
        state.players[survivor_b.0 as usize].is_eliminated,
        "survivor B is eliminated with the team"
    );
    assert!(
        matches!(
            state.waiting_for,
            WaitingFor::GameOver {
                winner: Some(HORDE)
            }
        ),
        "with the whole survivor team eliminated and the Horde undefeated, the Horde \
         wins via the archenemy path. waiting_for={:?}",
        state.waiting_for
    );
}

/// Setup turns: from a fresh Horde game the survivors take exactly
/// `survivor_setup_turns` (3) turns before the Horde takes its first turn. Driven
/// through the real `start_next_turn` turn-rotation seam.
///
/// Revert guard: without the `turns_to_skip` seeding in `GameState::new`, the
/// Horde's turn is NOT skipped, so the active player after the FIRST
/// `start_next_turn` would already be the Horde — the `actives[1] == survivor`
/// assertion flips.
#[test]
fn survivors_take_setup_turns_before_the_horde() {
    let mut state = horde_game(2);
    // The published Cyberman ruleset gives the survivors 3 setup turns.
    let setup_turns = state
        .format_config
        .horde_ruleset
        .as_ref()
        .unwrap()
        .survivor_setup_turns;
    assert_eq!(
        setup_turns, 3,
        "reach-guard: Cyberman survivor_setup_turns is 3"
    );

    // The survivor turn representative (lowest non-Horde seat).
    let survivor_rep = PlayerId(1);

    // Turn 1 is the initial state (starting_player is a survivor, PR1). Then drive
    // the next several turn boundaries and record who is active at each.
    let mut actives = vec![state.active_player];
    let mut events = Vec::new();
    for _ in 0..3 {
        start_next_turn(&mut state, &mut events);
        actives.push(state.active_player);
    }

    // The first `setup_turns` turns are all the survivor team; the (setup_turns+1)th
    // is the Horde's first turn.
    assert_eq!(
        actives[0], survivor_rep,
        "turn 1 (initial) belongs to the survivor team"
    );
    assert_eq!(
        actives[1], survivor_rep,
        "turn 2 is still the survivor team (the Horde's turn was skipped)"
    );
    assert_eq!(
        actives[2], survivor_rep,
        "turn 3 is still the survivor team"
    );
    assert_eq!(
        actives[3], HORDE,
        "turn 4 is the Horde's FIRST turn — after exactly 3 survivor setup turns"
    );

    // Exactly `survivor_setup_turns` survivor turns precede the Horde's first.
    let survivor_turns_before_horde = actives.iter().take_while(|&&p| p != HORDE).count();
    assert_eq!(
        survivor_turns_before_horde as u8, setup_turns,
        "survivors take exactly survivor_setup_turns turns before the Horde"
    );
}

/// Hostile: the Horde seat is NOT a member of the survivor shared-life team. The
/// Horde's life is never part of `team_life_total(survivor)`, and it is not a
/// teammate of any survivor.
///
/// Revert guard: if the Horde were folded into the survivor team, setting the
/// Horde's life to a large sentinel would inflate `team_life_total(survivor)` and
/// the `== 20` assertion would flip.
#[test]
fn horde_is_not_a_member_of_the_survivor_team() {
    let mut state = horde_game(2);
    let survivor_a = PlayerId(1);
    let survivor_b = PlayerId(2);

    // The Horde is not a teammate of any survivor.
    let survivor_a_teammates = teammates(&state, survivor_a);
    assert!(
        !survivor_a_teammates.contains(&HORDE),
        "the Horde must not be a teammate of a survivor"
    );
    assert!(
        survivor_a_teammates.contains(&survivor_b),
        "reach-guard: the other survivor IS a teammate (team membership is real)"
    );

    // Set the Horde's underlying life field to a large sentinel; the survivor team
    // total must be unaffected (the Horde's life is excluded).
    state.players[HORDE.0 as usize].life = 999;
    assert_eq!(
        team_life_total(&state, survivor_a),
        20,
        "the Horde's life is NOT part of the survivor team total"
    );
}

/// Regression: a SINGLE-survivor Horde game (player_count 2) still works — the
/// survivor's team total equals its own (unsplit) life, a degenerate team of one.
///
/// Revert guard: an off-by-one in the distribution (e.g. dividing by the wrong
/// count) would change the sole survivor's life away from the combined 20.
#[test]
fn single_survivor_team_total_equals_own_life() {
    let state = horde_game(1);
    let survivor = PlayerId(1);

    assert_eq!(
        state.players[survivor.0 as usize].life, 20,
        "a lone survivor keeps the full combined 20"
    );
    assert_eq!(
        team_life_total(&state, survivor),
        20,
        "a degenerate team of one reads its own life as the team total"
    );
    // The Horde's default life is untouched by the distribution (it has no life total).
    assert_eq!(
        state.players[HORDE.0 as usize].life, state.format_config.starting_life,
        "the Horde seat's life field is left at its seeded value (never consulted)"
    );
}
