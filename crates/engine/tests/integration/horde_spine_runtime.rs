//! Horde Magic spine-runtime integration tests (PR2).
//!
//! These drive the REAL turn/priority/cast pipeline through `GameRunner`:
//!   - the Horde skips its draw step every turn;
//!   - the Horde's precombat main reveals-and-casts a `FixedCount(k)` wave of
//!     nontoken cards, which RESOLVE onto the battlefield under the Horde;
//!   - the game-start emblem grants haste + must-attack to the Horde's
//!     creatures (and not to survivor-controlled creatures);
//!   - a survivor's precombat main fires no wave.
//!
//! Each test names an assertion that flips if the corresponding wiring is
//! reverted (see the per-test doc comments).

use engine::game::deck_loading::grant_horde_emblem;
use engine::game::functioning_abilities::active_static_definitions;
use engine::game::keywords::has_haste;
use engine::game::layers::flush_layers;
use engine::game::scenario::GameRunner;
use engine::game::zones::create_object;
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::format::{ChallengeDeck, FormatConfig, HordeRuleset, WaveTermination};
use engine::types::game_state::{GameState, WaitingFor};
use engine::types::identifiers::{CardId, ObjectId};
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::statics::StaticMode;
use engine::types::zones::Zone;

const HORDE: PlayerId = PlayerId(0);
const SURVIVOR: PlayerId = PlayerId(1);

fn ruleset(wave: u32, forced_attackers: bool) -> HordeRuleset {
    let mut r = ChallengeDeck::CybermanHorde.default_ruleset();
    r.wave = WaveTermination::FixedCount(wave);
    r.horde_creatures_forced_attackers = forced_attackers;
    r
}

/// A Horde game (seat 0 = Horde, seat 1 = survivor) parked at the given active
/// player's Upkeep with priority, on a mid-game turn (turn 4 avoids any
/// first-turn draw-skip interaction).
fn horde_game(wave: u32, active: PlayerId) -> GameState {
    let mut state = GameState::new(FormatConfig::horde(ruleset(wave, true)), 2, 42);
    state.turn_number = 4;
    state.active_player = active;
    state.phase = Phase::Upkeep;
    state.priority_player = active;
    state.waiting_for = WaitingFor::Priority { player: active };
    state
}

/// Put a vanilla castable creature on top of `owner`'s library. Returns its id.
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

fn battlefield_creatures_controlled_by(state: &GameState, controller: PlayerId) -> Vec<ObjectId> {
    state
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            state.objects.get(id).is_some_and(|o| {
                o.controller == controller && o.card_types.core_types.contains(&CoreType::Creature)
            })
        })
        .collect()
}

/// The Horde skips its draw step every turn. With a `FixedCount(0)` wave (so no
/// reveal disturbs the library), advancing the Horde through its draw step into
/// precombat main must leave the Horde's hand empty and its library untouched.
///
/// Revert guard: dropping `is_horde_turn` from the draw-step gate lets the Horde
/// draw — `hand` would then hold 1 card and the library would shrink by 1.
#[test]
fn horde_skips_its_draw_step() {
    let mut state = horde_game(0, HORDE);
    for i in 0..3 {
        add_library_creature(&mut state, HORDE, &format!("Horde Card {i}"), 2);
    }
    let library_before = state.players[HORDE.0 as usize].library.len();

    let mut runner = GameRunner::from_state(state);
    runner.advance_to_phase(Phase::PreCombatMain);

    assert_eq!(
        runner.state().phase,
        Phase::PreCombatMain,
        "the Horde turn must reach precombat main"
    );
    assert!(
        runner.state().players[HORDE.0 as usize].hand.is_empty(),
        "the Horde must not draw — its hand must stay empty across the draw step"
    );
    assert_eq!(
        runner.state().players[HORDE.0 as usize].library.len(),
        library_before,
        "no card may leave the Horde's library via a (skipped) draw"
    );
}

/// On the Horde's precombat main, a `FixedCount(k)` wave reveals-and-casts
/// exactly `k` nontoken cards, which RESOLVE onto the battlefield under the
/// Horde (not left on the stack).
///
/// Positive reach-guard: the `k` cards actually entered the battlefield under
/// the Horde's control. Revert guard: removing the `finish_enter_phase` /
/// executor dispatch (or the priority-seam re-entry) leaves 0 (or <k) creatures
/// on the battlefield.
#[test]
fn horde_precombat_main_reveals_and_resolves_fixed_wave() {
    const K: u32 = 2;
    let mut state = horde_game(K, HORDE);
    // Three cards available; only K should be revealed-and-cast this wave.
    for i in 0..3 {
        add_library_creature(&mut state, HORDE, &format!("Cyberman {i}"), 2);
    }
    let library_before = state.players[HORDE.0 as usize].library.len();

    let mut runner = GameRunner::from_state(state);
    runner.advance_to_phase(Phase::PreCombatMain);

    // Drive priority until the whole wave has resolved (bounded).
    for _ in 0..40 {
        let done = battlefield_creatures_controlled_by(runner.state(), HORDE).len() >= K as usize
            && runner.state().stack.is_empty()
            && runner.state().horde_wave_remaining == 0;
        if done {
            break;
        }
        if runner.act(GameAction::PassPriority).is_err() {
            break;
        }
    }

    let horde_creatures = battlefield_creatures_controlled_by(runner.state(), HORDE);
    assert_eq!(
        horde_creatures.len(),
        K as usize,
        "exactly {K} nontoken cards must resolve onto the battlefield under the Horde, got {}",
        horde_creatures.len()
    );
    assert!(
        runner.state().stack.is_empty(),
        "the revealed spells must RESOLVE, not remain on the stack"
    );
    assert_eq!(
        runner.state().horde_wave_remaining,
        0,
        "the wave counter must be exhausted after k reveals"
    );
    // Only K of the three library cards were consumed by the wave.
    assert_eq!(
        runner.state().players[HORDE.0 as usize].library.len(),
        library_before - K as usize,
        "a FixedCount(k) wave consumes exactly k library cards"
    );
}

/// The game-start Horde emblem grants haste and "attacks each combat if able"
/// (must-attack) to creatures the Horde controls — a live-controller grant, so a
/// survivor-controlled creature receives neither.
///
/// Revert guard (haste): dropping `AddKeyword(Haste)` from the emblem static
/// makes `has_haste` false. Revert guard (must-attack): dropping the
/// `GrantStaticAbility(MustAttack)` modification removes the MustAttack static.
#[test]
fn horde_emblem_grants_haste_and_must_attack_to_horde_creatures_only() {
    let mut state = horde_game(0, HORDE);
    grant_horde_emblem(&mut state, HORDE, true);

    let horde_creature = create_object(
        &mut state,
        CardId(9001),
        HORDE,
        "Dalek".to_string(),
        Zone::Battlefield,
    );
    let survivor_creature = create_object(
        &mut state,
        CardId(9002),
        SURVIVOR,
        "Human".to_string(),
        Zone::Battlefield,
    );
    for id in [horde_creature, survivor_creature] {
        let obj = state.objects.get_mut(&id).unwrap();
        obj.card_types.core_types.push(CoreType::Creature);
        obj.base_card_types = obj.card_types.clone();
        obj.power = Some(2);
        obj.toughness = Some(2);
        obj.base_power = Some(2);
        obj.base_toughness = Some(2);
    }

    flush_layers(&mut state);

    let horde_obj = state.objects.get(&horde_creature).unwrap();
    assert!(
        has_haste(horde_obj),
        "the Horde's creature must have haste from the emblem"
    );
    assert!(
        active_static_definitions(&state, horde_obj).any(|sd| sd.mode == StaticMode::MustAttack),
        "the Horde's creature must have a granted MustAttack static from the emblem"
    );

    // Hostile: a survivor-controlled creature gets neither grant (live-controller
    // `ControllerRef::You` scoped to the Horde emblem's controller).
    let survivor_obj = state.objects.get(&survivor_creature).unwrap();
    assert!(
        !has_haste(survivor_obj),
        "a survivor-controlled creature must NOT gain haste from the Horde emblem"
    );
    assert!(
        !active_static_definitions(&state, survivor_obj)
            .any(|sd| sd.mode == StaticMode::MustAttack),
        "a survivor-controlled creature must NOT gain the MustAttack static"
    );
}

/// `horde_creatures_forced_attackers = false` grants haste but NOT must-attack,
/// proving the ruleset flag gates only the MustAttack modification.
#[test]
fn horde_emblem_respects_forced_attackers_flag() {
    let mut state = horde_game(0, HORDE);
    grant_horde_emblem(&mut state, HORDE, false);

    let creature = create_object(
        &mut state,
        CardId(9003),
        HORDE,
        "Cyberman".to_string(),
        Zone::Battlefield,
    );
    {
        let obj = state.objects.get_mut(&creature).unwrap();
        obj.card_types.core_types.push(CoreType::Creature);
        obj.base_card_types = obj.card_types.clone();
        obj.power = Some(1);
        obj.toughness = Some(1);
        obj.base_power = Some(1);
        obj.base_toughness = Some(1);
    }
    flush_layers(&mut state);

    let obj = state.objects.get(&creature).unwrap();
    assert!(
        has_haste(obj),
        "haste is granted regardless of the forced-attackers flag"
    );
    assert!(
        !active_static_definitions(&state, obj).any(|sd| sd.mode == StaticMode::MustAttack),
        "must-attack must be gated off when horde_creatures_forced_attackers is false"
    );
}

/// Hostile: on a SURVIVOR's precombat main no wave fires — the Horde's library
/// is untouched and no Horde creature appears. Guards that the wave is scoped to
/// the Horde's own turn (`is_horde_turn`).
#[test]
fn survivor_precombat_main_fires_no_wave() {
    let mut state = horde_game(2, SURVIVOR);
    for i in 0..3 {
        add_library_creature(&mut state, HORDE, &format!("Horde Card {i}"), 2);
    }
    // Give the survivor a few cards so its normal draw step doesn't deck it out.
    for i in 0..3 {
        add_library_creature(&mut state, SURVIVOR, &format!("Survivor Card {i}"), 2);
    }
    let horde_library_before = state.players[HORDE.0 as usize].library.len();

    let mut runner = GameRunner::from_state(state);
    runner.advance_to_phase(Phase::PreCombatMain);

    assert_eq!(
        runner.state().phase,
        Phase::PreCombatMain,
        "the survivor turn must reach precombat main normally"
    );
    assert_eq!(
        runner.state().horde_wave_remaining,
        0,
        "no wave counter may be seeded on a survivor's turn"
    );
    assert_eq!(
        runner.state().players[HORDE.0 as usize].library.len(),
        horde_library_before,
        "the Horde's library must be untouched on a survivor's turn"
    );
    assert!(
        battlefield_creatures_controlled_by(runner.state(), HORDE).is_empty(),
        "no Horde creature may enter on a survivor's turn"
    );
}
