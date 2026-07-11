//! Horde Magic PR4 — the token-in-library primitive.
//!
//! Tokens can live in the Horde's library and ENTER THE BATTLEFIELD when the
//! Horde reveals them (a token is never cast — CR 111). Two pieces are exercised:
//!
//!   1. `GameObject.in_horde_library` EXEMPTS a library-resident token from the
//!      CR 704.5d token cease-to-exist state-based action, so it can sit in the
//!      library without being swept.
//!   2. On the Horde's precombat main, revealing a library token creates a FRESH
//!      battlefield token under the Horde's control (CR 111.1 + CR 111.2) and
//!      removes the library placeholder so no duplicate remains.
//!
//! Seat 0 = Horde, seat 1 = survivor. Every negative assertion is paired with a
//! positive reach-guard proving the input reached the seam under test.

use engine::game::sba::check_state_based_actions;
use engine::game::scenario::GameRunner;
use engine::game::zones::{create_object, move_to_zone};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::format::{ChallengeDeck, FormatConfig, WaveTermination};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;

const HORDE: PlayerId = PlayerId(0);

/// A Horde game (seat 0 = Horde) with a `FixedCount(wave)` wave, parked at the
/// Horde's Upkeep with priority on a mid-game turn (turn 4 avoids any
/// first-turn draw-skip interaction). Mirrors `horde_spine_runtime`'s fixture.
fn horde_game(wave: u32) -> GameState {
    let mut ruleset = ChallengeDeck::CybermanHorde.default_ruleset();
    ruleset.wave = WaveTermination::FixedCount(wave);
    let mut state = GameState::new(FormatConfig::horde(ruleset), 2, 42);
    state.turn_number = 4;
    state.active_player = HORDE;
    state.phase = Phase::Upkeep;
    state.priority_player = HORDE;
    state.waiting_for = WaitingFor::Priority { player: HORDE };
    state
}

/// Place a creature token on top of `owner`'s library. `flagged` sets the new
/// `in_horde_library` cease-to-exist exemption.
fn add_library_token(
    state: &mut GameState,
    owner: PlayerId,
    name: &str,
    p: i32,
    t: i32,
    flagged: bool,
) -> ObjectId {
    let card_id = CardId(state.next_object_id);
    let id = create_object(state, card_id, owner, name.to_string(), Zone::Library);
    let obj = state.objects.get_mut(&id).unwrap();
    obj.is_token = true;
    obj.in_horde_library = flagged;
    obj.card_types.core_types.push(CoreType::Creature);
    obj.base_card_types = obj.card_types.clone();
    obj.power = Some(p);
    obj.toughness = Some(t);
    obj.base_power = Some(p);
    obj.base_toughness = Some(t);
    id
}

/// Place a vanilla castable (nontoken) creature on top of `owner`'s library.
fn add_library_creature(state: &mut GameState, owner: PlayerId, name: &str, mv: u32) -> ObjectId {
    let card_id = CardId(state.next_object_id);
    let id = create_object(state, card_id, owner, name.to_string(), Zone::Library);
    let obj = state.objects.get_mut(&id).unwrap();
    obj.card_types.core_types.push(CoreType::Creature);
    obj.base_card_types = obj.card_types.clone();
    obj.mana_cost = ManaCost::Cost {
        shards: vec![],
        generic: mv,
    };
    obj.power = Some(mv as i32);
    obj.toughness = Some(mv as i32);
    obj.base_power = Some(mv as i32);
    obj.base_toughness = Some(mv as i32);
    id
}

fn horde_battlefield_tokens(state: &GameState) -> Vec<ObjectId> {
    state
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            state
                .objects
                .get(id)
                .is_some_and(|o| o.controller == HORDE && o.is_token)
        })
        .collect()
}

/// Run the Horde's precombat-main wave to completion (bounded).
fn run_wave(runner: &mut GameRunner) {
    runner.advance_to_phase(Phase::PreCombatMain);
    for _ in 0..40 {
        if runner.state().horde_wave_remaining == 0 && runner.state().stack.is_empty() {
            break;
        }
        if runner.act(GameAction::PassPriority).is_err() {
            break;
        }
    }
}

/// A token flagged `in_horde_library` in the Horde's library SURVIVES a
/// state-based-action pass; an otherwise-identical UNFLAGGED off-battlefield
/// token IS swept.
///
/// Revert guard: dropping the `!obj.in_horde_library` clause in
/// `check_token_cease_to_exist` sweeps the flagged token too — the survival
/// assertion flips. Positive reach-guard: the unflagged token being swept proves
/// the CR 704.5d SBA actually ran (so the survival is the exemption's doing, not
/// an inert SBA).
#[test]
fn flagged_library_token_survives_sba_unflagged_is_swept() {
    let mut state = horde_game(0);
    let flagged = add_library_token(&mut state, HORDE, "Cyberman", 2, 2, true);
    let unflagged = add_library_token(&mut state, HORDE, "Dalek", 3, 3, false);

    let mut events: Vec<GameEvent> = Vec::new();
    check_state_based_actions(&mut state, &mut events);

    assert!(
        state.objects.contains_key(&flagged),
        "an in_horde_library token must survive the CR 704.5d cease-to-exist SBA"
    );
    assert!(
        state.players[HORDE.0 as usize].library.contains(&flagged),
        "the flagged token must remain in the Horde's library"
    );
    // Positive reach-guard: the SBA is reachable and sweeps unflagged tokens.
    assert!(
        !state.objects.contains_key(&unflagged),
        "an UNFLAGGED off-battlefield token MUST be swept — proves the exemption \
         is scoped to the flag and the SBA is actually running"
    );
}

/// On the Horde's precombat main, revealing a library token puts a token onto the
/// BATTLEFIELD under the Horde's control and REMOVES the library placeholder (no
/// duplicate; library count -1).
///
/// Revert guard: restoring the old graveyard stub (no CreateToken) leaves the
/// battlefield token count at 0. Dropping the library-object removal leaves the
/// placeholder object present and the library count unchanged.
#[test]
fn precombat_main_reveal_puts_library_token_onto_battlefield() {
    let mut state = horde_game(1);
    let placeholder = add_library_token(&mut state, HORDE, "Cyberman", 2, 2, true);
    let library_before = state.players[HORDE.0 as usize].library.len();

    let mut runner = GameRunner::from_state(state);
    run_wave(&mut runner);

    let bf_tokens = horde_battlefield_tokens(runner.state());
    assert_eq!(
        bf_tokens.len(),
        1,
        "revealing a library token must put exactly one token onto the battlefield under the Horde"
    );
    let bf = runner.state().objects.get(&bf_tokens[0]).unwrap();
    assert_eq!(
        (bf.power, bf.toughness),
        (Some(2), Some(2)),
        "the battlefield token must carry the revealed body's P/T"
    );
    assert!(
        !runner.state().objects.contains_key(&placeholder),
        "the library placeholder object must be removed — no duplicate may remain"
    );
    assert_eq!(
        runner.state().players[HORDE.0 as usize].library.len(),
        library_before - 1,
        "the Horde's library count must decrement by exactly 1"
    );
}

/// The revealed battlefield token is a NORMAL token: it does NOT carry
/// `in_horde_library`, so it ceases to exist normally (CR 704.5d) if it later
/// leaves the battlefield.
///
/// Revert guard: if the reveal path erroneously stamped `in_horde_library` on the
/// fresh token, the flag assertion flips AND the token would survive leaving the
/// battlefield (the final cease-to-exist assertion flips too).
#[test]
fn revealed_battlefield_token_is_normal_and_ceases_when_it_leaves() {
    let mut state = horde_game(1);
    add_library_token(&mut state, HORDE, "Cyberman", 2, 2, true);

    let mut runner = GameRunner::from_state(state);
    run_wave(&mut runner);

    let bf_tokens = horde_battlefield_tokens(runner.state());
    assert_eq!(bf_tokens.len(), 1, "one battlefield token expected");
    let bf = bf_tokens[0];
    assert!(
        !runner.state().objects.get(&bf).unwrap().in_horde_library,
        "the fresh battlefield token must NOT carry in_horde_library"
    );

    // Sanity: it is a normal token now — move it off the battlefield and the
    // CR 704.5d SBA sweeps it (the exemption no longer applies).
    let mut events: Vec<GameEvent> = Vec::new();
    move_to_zone(runner.state_mut(), bf, Zone::Graveyard, &mut events);
    check_state_based_actions(runner.state_mut(), &mut events);
    assert!(
        !runner.state().objects.contains_key(&bf),
        "a normal token must cease to exist once it leaves the battlefield"
    );
}

/// Hostile mixed wave: a `FixedCount(2)` wave over [token, nontoken] yields one
/// battlefield token PLUS one resolved nontoken permanent, both under the Horde.
/// Exercises the synchronous-token → continue-to-nontoken loop in
/// `maybe_reveal_next` (the token cannot pause the wave via the stack).
///
/// Revert guard: reverting the token branch to the graveyard stub drops the
/// token permanent (token count 0). Reverting the loop (returning `None` after a
/// synchronous token) strands the nontoken, which never resolves.
#[test]
fn mixed_wave_yields_one_token_and_one_nontoken_under_horde() {
    let mut state = horde_game(2);
    // `create_object` pushes to the back of the library and the wave reveals the
    // front first, so the token (created first) is revealed before the nontoken.
    add_library_token(&mut state, HORDE, "Dalek", 3, 3, true);
    add_library_creature(&mut state, HORDE, "Cyber Controller", 2);

    let mut runner = GameRunner::from_state(state);
    run_wave(&mut runner);

    let horde_creatures: Vec<ObjectId> = runner
        .state()
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            runner.state().objects.get(id).is_some_and(|o| {
                o.controller == HORDE && o.card_types.core_types.contains(&CoreType::Creature)
            })
        })
        .collect();

    let tokens = horde_creatures
        .iter()
        .filter(|id| runner.state().objects.get(id).unwrap().is_token)
        .count();
    let nontokens = horde_creatures
        .iter()
        .filter(|id| !runner.state().objects.get(id).unwrap().is_token)
        .count();

    assert_eq!(
        tokens, 1,
        "the token half of the wave must yield exactly one battlefield token"
    );
    assert_eq!(
        nontokens, 1,
        "the nontoken half of the wave must resolve exactly one permanent (loop reached it)"
    );
}
