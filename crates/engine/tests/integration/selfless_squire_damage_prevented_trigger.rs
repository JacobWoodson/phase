//! CR 615.13 — "Whenever damage that would be dealt to you is prevented" must
//! actually fire, and must read the amount of THAT application.
//!
//! Selfless Squire ({3}{W} Creature — Human Soldier 1/1) is the corpus's one
//! standalone prevention-trigger card. At base its second line parsed to
//! `TriggerMode::Unknown` and its matcher was `match_unimplemented` (a hard
//! `return false`), so the trigger never fired even though its execute payload
//! (`PutCounter{P1P1, Ref(EventContextAmount), SelfRef}`) was already correct.
//!
//! CR 615.13: "Some triggered abilities trigger when damage that would be dealt
//! is prevented. Such an ability triggers each time a prevention effect is
//! applied to one or more simultaneous damage events and prevents some or all of
//! that damage."
//!
//! The "once" in `DamagePreventedOnce` is a property of the EVENT STREAM, not of
//! the matcher: the combat path emits ONE aggregate `DamagePrevented` per shield
//! per simultaneous batch, and the non-combat path emits one per event. The
//! matcher is therefore a pure per-event predicate and adds no dedup or
//! once-per-turn latch — `two_separate_preventions_each_read_their_own_amount`
//! pins that two separate applications produce two triggers, each reading its own
//! amount.
//!
//! THE COMBAT HALF, and how CR 615.13 and CR 120.3 are BOTH honored. The combat
//! path aggregates a whole CR 510.2 simultaneous batch before emitting, and it
//! used to re-derive the prevented target from the shield's own filter — mapping
//! only the `Specific(PlayerId)` scope to a player, so a controller-scoped
//! ("dealt to you") shield emitted an event naming the shield OBJECT and the
//! matcher correctly refused it under CR 120.3. The recipient is now THREADED
//! from the replacement applier that prevented the damage
//! (`game/replacement.rs`'s tally writes → `PreventedDamageEntry` →
//! `game/combat_damage.rs::fire_combat_prevention_riders`), never re-derived,
//! because a shield's `damage_target_filter` is a PREDICATE over recipients and
//! not the recipient of any one event. So:
//!
//!   * CR 615.13 fixes the TRIGGER/rider unit — one prevention APPLICATION per
//!     simultaneous batch, so the CR 615.5 rider fires once against the batch
//!     total;
//!   * CR 120.3 fixes the RECIPIENT — one `DamagePrevented` per (shield,
//!     recipient), each carrying that recipient's own amount.
//!
//! Safe Passage is the fixture that makes the split visible: one application,
//! two recipients (the player for 2, a creature for 3), and a player-scoped
//! trigger must read 2 — not 5, and not 3.
//!
//! KNOWN RESIDUALS, stated rather than hidden:
//!   * the aggregate's `source_id` is the SHIELD's host object (sentinel
//!     `ObjectId(0)` when floating), not the damage source — a pinned convention,
//!     because CR 615.13's application spans simultaneous events that under
//!     CR 510.2 routinely have DIFFERENT sources. No test in this file may assert
//!     `source_id` as if it were the damage source;
//!   * per-source-reflecting shields (Comeuppance) bypass the batch tally
//!     entirely and still emit one event per damage event, over-firing the
//!     trigger count under CR 615.13. That is pre-existing and out of scope;
//!     `reflecting_shields_still_emit_per_damage_event` characterizes it so it
//!     cannot change silently. CR 615.5 binds each reflection to its own damage
//!     source, so it cannot simply be batched;
//!   * `PreventionAmount::Next(N)` / `AllBut(N)` likewise never enter the tally —
//!     populations (1)/(2) of the three per-event-bypass populations documented
//!     in `replacement.rs`. `depletion_shields_still_emit_per_damage_event`
//!     characterizes the `Next(N)` member the same way the `reflecting_...` row
//!     covers population (3), so it cannot change silently either. CR 615.7's
//!     simultaneous-damage choice is about WHICH damage the shield prevents,
//!     not about emitting per event, so it does not defend the shape.
//!
//! The positive tests are discriminating: reverting the matcher
//! (`match_damage_prevented` → `match_unimplemented`) or the parser arm (mode →
//! `Unknown`) drops every counter assertion to 0. Each negative carries a paired
//! positive reach-guard in the SAME test, because "0 counters" is also what "the
//! trigger was never registered at all" looks like.
//!
//! Oracle text is verbatim from Scryfall.

use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    DamageTargetFilter, DamageTargetPlayerScope, PreventionAmount, ShieldKind, TargetRef,
};
use engine::types::actions::GameAction;
use engine::types::counter::CounterType;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;

/// Verbatim Selfless Squire (Scryfall). The second line is the trigger under test.
const SELFLESS_SQUIRE_TEXT: &str = "Flash\nWhen this creature enters, prevent all damage that would be dealt to you this turn.\nWhenever damage that would be dealt to you is prevented, put that many +1/+1 counters on this creature.";

/// Verbatim Solitary Confinement — a printed static shield with no turn window,
/// used as an INDEPENDENT prevention source. Deliberately not the Squire's own
/// ETB shield: the trigger must observe prevention from any source, not only from
/// a shield it created.
const SOLITARY_CONFINEMENT_TEXT: &str = "Skip your draw step.\nAt the beginning of your upkeep, sacrifice Solitary Confinement unless you discard a card.\nPrevent all damage that would be dealt to you.";

/// Verbatim Energy Field (`data/card-data.json`). A PRINTED `Prevention{All}` /
/// `Player{Controller}` shield — the 14-card class the combat aggregate used to
/// mis-attribute to the shield object. (The raw census over the 35,804-card
/// corpus finds 22 such nodes; the other 8 carry a non-null `redirect_target` and
/// are CR 614.9 REDIRECTIONS, not CR 615 preventions — redirection deals the same
/// damage to another recipient, so nothing is prevented and they never reach the
/// tally at all.)
///
/// Chosen over this file's `SOLITARY_CONFINEMENT_TEXT` constant, which is NOT
/// verbatim: it reorders the printed lines, names the card instead of "this
/// enchantment", and drops "You have shroud."
const ENERGY_FIELD_TEXT: &str = "Prevent all damage that would be dealt to you by sources you don't control.\nWhen a card is put into your graveyard from anywhere, sacrifice this enchantment.";

/// Verbatim Safe Passage. THE multi-recipient shield: one CR 615.13 application,
/// a player leg and a creature leg in the same CR 510.2 simultaneous batch.
const SAFE_PASSAGE_TEXT: &str =
    "Prevent all damage that would be dealt to you and creatures you control this turn.";

/// Verbatim Riot Control. A resolution-created player shield: `TargetFilter::
/// Controller` is a context ref, so `untargeted_damage_filter` lowers it to
/// `Player { Specific(P0) }` — the one scope the deleted re-derivation used to
/// map, which is why this row must keep passing.
const RIOT_CONTROL_TEXT: &str = "You gain 1 life for each creature your opponents control. Prevent all damage that would be dealt to you this turn.";

/// Verbatim Fog. `TargetFilter::Any` lowers to NO `damage_target_filter` at all,
/// so there is no scope for a filter-derived recipient to come from — the old
/// path fabricated `Object(ObjectId(0))`, an object that does not exist.
const FOG_TEXT: &str = "Prevent all combat damage that would be dealt this turn.";

/// Verbatim Comeuppance. Per-source-reflecting, so it bypasses the batch tally
/// entirely and keeps the pre-existing per-damage-event emission.
const COMEUPPANCE_TEXT: &str = "Prevent all damage that would be dealt to you and planeswalkers you control this turn by sources you don't control. If damage from a creature source is prevented this way, Comeuppance deals that much damage to that creature. If damage from a noncreature source is prevented this way, Comeuppance deals that much damage to the source's controller.";

/// SYNTHETIC minimal depletion shield. No printed card carries exactly this
/// line; the SHAPE is grammar-real — `PreventionAmount::Next` with a
/// controller-relative "dealt to you" target — composed from two parser-handled
/// halves: Test of Faith's "Prevent the next 3 damage" amount grammar and Riot
/// Control's "dealt to you" target grammar. `Next(7)` overcovers the 2-and-3
/// batch with 2 to spare, so no CR 615.7 apportionment choice can be blamed for
/// anything the characterization row observes.
const DEPLETION_BULWARK_TEXT: &str =
    "Prevent the next 7 damage that would be dealt to you this turn.";

fn free_cost() -> ManaCost {
    ManaCost::Cost {
        shards: vec![],
        generic: 0,
    }
}

/// Stock both libraries. Without this a player decks out, the game ends in
/// `WaitingFor::GameOver`, combat never runs, and EVERY counter assertion passes
/// vacuously at 0. Borrowed from `printed_damage_prevention_survives_turn.rs`.
fn stock_libraries(scenario: &mut GameScenario) {
    scenario.with_library_top(
        P0,
        &["F0a", "F0b", "F0c", "F0d", "F0e", "F0f", "F0g", "F0h"],
    );
    scenario.with_library_top(
        P1,
        &["F1a", "F1b", "F1c", "F1d", "F1e", "F1f", "F1g", "F1h"],
    );
}

/// Put Selfless Squire on the battlefield with its verbatim Oracle text.
fn add_squire(scenario: &mut GameScenario, player: PlayerId) -> ObjectId {
    scenario
        .add_creature_from_oracle(player, "Selfless Squire", 1, 1, SELFLESS_SQUIRE_TEXT)
        .id()
}

/// Add a free enchantment carrying a printed static prevention line. A
/// permanent-type seed MUST precede the Oracle text — `parse_oracle_text` given
/// `types: ["Sorcery"]` returns zero replacements for a static prevention line.
fn add_shield_enchantment(
    scenario: &mut GameScenario,
    player: PlayerId,
    name: &str,
    text: &str,
) -> ObjectId {
    scenario
        .add_spell_to_hand(player, name, false)
        .as_enchantment()
        .from_oracle_text(text)
        .with_mana_cost(free_cost())
        .id()
}

/// Add a free INSTANT carrying a prevention line, for the rows that need the
/// shield to exist only after attackers are declared (CR 615.4).
fn add_instant_from_oracle(
    scenario: &mut GameScenario,
    player: PlayerId,
    name: &str,
    text: &str,
) -> ObjectId {
    scenario
        .add_spell_to_hand_from_oracle(player, name, true, text)
        .with_mana_cost(free_cost())
        .id()
}

/// Advance until `target` is the active player, returning whether that happened.
///
/// `GameRunner::advance_to_phase` cannot be used here: it breaks out of its loop
/// on ANY non-`Priority` waiting state, and every test in this file puts a
/// creature (the Squire) on P0's battlefield, so the engine surfaces the CR 508.1
/// declare-attackers turn-based action on P0's own turn and the advance silently
/// stalls at `DeclareAttackers`. This helper answers those turn-based prompts
/// (declaring no attacks and no blocks) so the turn can actually roll over.
///
/// The bool is the stall guard — callers MUST assert it. Every counter assertion
/// downstream reads "N counters", which is also what "the turn never advanced and
/// combat never happened" looks like.
#[must_use = "the turn crossing must be asserted to have happened — see doc comment"]
fn pass_turn_to(runner: &mut GameRunner, target: PlayerId) -> bool {
    for _ in 0..600 {
        if runner.state().active_player == target && runner.state().phase == Phase::PreCombatMain {
            return true;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::Priority { .. } => {
                if runner.act(GameAction::PassPriority).is_err() {
                    return false;
                }
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                if runner.act(GameAction::OrderTriggers { order }).is_err() {
                    return false;
                }
            }
            WaitingFor::DeclareAttackers { .. } => {
                if runner.declare_attackers(&[]).is_err() {
                    return false;
                }
            }
            WaitingFor::DeclareBlockers { .. } => {
                if runner.declare_blockers(&[]).is_err() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    false
}

/// Drive combat, declaring `attackers` against `defend_player`, and COLLECT every
/// event the drive produced.
///
/// The single combat driver for this file. It replaced a no-blocks,
/// no-event-log variant (`run_combat_unblocked`) whose only caller was the
/// characterization test that
/// `combat_prevention_from_a_printed_player_shield_puts_that_many_counters`
/// converted; keeping a delegating wrapper with no callers is dead code, so the
/// three capabilities were folded in here instead:
///
///   * every `ActionResult`'s events are appended to a log, so a test can assert
///     the exact `DamagePrevented` list the CR 510.2 batch emitted (length
///     included — a spurious `amount: 0` entry must surface as an extra element);
///   * `WaitingFor::DeclareBlockers` submits `blocks` (`(blocker, attacker)` per
///     CR 509.1a) instead of declaring none, so a batch can carry BOTH a player
///     recipient and a creature recipient;
///   * `flash`, if supplied, is cast at the first priority window seen AFTER
///     attackers were declared — the CR 615.4 window in which an instant-speed
///     prevention shield must exist before the damage event occurs.
///
/// The bool is the same combat reach-guard and MUST be asserted; it additionally
/// covers `flash` having actually been cast.
#[must_use = "combat must be asserted to have actually run — see doc comment"]
fn run_combat_collecting(
    runner: &mut GameRunner,
    attacker_player: PlayerId,
    attackers: &[ObjectId],
    defend_player: PlayerId,
    blocks: &[(ObjectId, ObjectId)],
    flash: Option<ObjectId>,
) -> (bool, Vec<GameEvent>) {
    let mut attacked = false;
    let mut reached_end_of_combat = false;
    let mut blocked = false;
    let mut flashed = flash.is_none();
    let mut log: Vec<GameEvent> = Vec::new();
    // The flash card's controller. `GameRunner::cast` routes through
    // `apply_as_current`, which acts as WHOEVER currently holds priority — and the
    // active player (the attacker) gets priority first in the declare-attackers
    // step. Casting a card out of the defender's hand while the attacker holds
    // priority is refused by `castable_from_current_zone` with
    // `InvalidAction("Card is not in a castable zone")`, so the cast must wait for
    // this player's own window.
    let flash_controller = flash.map(|spell| runner.state().objects[&spell].controller);

    for _ in 0..400 {
        if matches!(
            runner.state().phase,
            Phase::EndCombat | Phase::PostCombatMain
        ) {
            reached_end_of_combat = true;
            break;
        }
        match runner.state().waiting_for.clone() {
            // CR 615.4: the shield has to exist BEFORE the damage event, so it is
            // cast in the first priority window its controller gets after
            // attackers are declared.
            WaitingFor::Priority { player, .. }
                if attacked && !flashed && Some(player) == flash_controller =>
            {
                flashed = true;
                let spell = flash.expect("`flashed` starts true when there is nothing to flash");
                let outcome = runner.cast(spell).resolve();
                log.extend(outcome.events().iter().cloned());
            }
            WaitingFor::Priority { .. } => match runner.act(GameAction::PassPriority) {
                Ok(result) => log.extend(result.events),
                Err(_) => break,
            },
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                match runner.act(GameAction::OrderTriggers { order }) {
                    Ok(result) => log.extend(result.events),
                    Err(_) => break,
                }
            }
            WaitingFor::DeclareAttackers { player, .. }
                if player == attacker_player && !attacked =>
            {
                attacked = true;
                let decl: Vec<_> = attackers
                    .iter()
                    .map(|a| (*a, AttackTarget::Player(defend_player)))
                    .collect();
                let result = runner
                    .declare_attackers(&decl)
                    .expect("declaring the intended attackers must succeed");
                log.extend(result.events);
            }
            WaitingFor::DeclareAttackers { .. } => match runner.declare_attackers(&[]) {
                Ok(result) => log.extend(result.events),
                Err(_) => break,
            },
            WaitingFor::DeclareBlockers { .. } => {
                let assignments: &[(ObjectId, ObjectId)] = if blocked { &[] } else { blocks };
                blocked = true;
                match runner.declare_blockers(assignments) {
                    Ok(result) => log.extend(result.events),
                    Err(_) => break,
                }
            }
            _ => break,
        }
    }

    // `flashed` is part of the reach-guard: a flash that never found its
    // controller's priority window leaves NO shield, and "0 counters" is exactly
    // what that looks like.
    (attacked && reached_end_of_combat && flashed, log)
}

/// Every `DamagePrevented` in `events`, reduced to the two fields this file is
/// allowed to assert.
///
/// `source_id` is deliberately DROPPED: on the combat aggregate it is the SHIELD's
/// host object (the sentinel `ObjectId(0)` for a floating shield), a pinned
/// convention rather than the damage source — see the module header. Asserting it
/// would read as a claim about the damage source that this change does not make.
fn prevented_pairs(events: &[GameEvent]) -> Vec<(TargetRef, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::DamagePrevented { target, amount, .. } => Some((target.clone(), *amount)),
            _ => None,
        })
        .collect()
}

/// `prevented_pairs`, sorted, for rows whose recipients have no required order.
fn sorted_prevented_pairs(events: &[GameEvent]) -> Vec<(TargetRef, u32)> {
    let mut pairs = prevented_pairs(events);
    pairs.sort();
    pairs
}

fn counters_on(runner: &GameRunner, obj: ObjectId) -> u32 {
    runner
        .state()
        .objects
        .get(&obj)
        .and_then(|o| o.counters.get(&CounterType::Plus1Plus1).copied())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// T1 — non-combat prevention, concrete amount (charter rows 2 + 5)
// ---------------------------------------------------------------------------

/// CR 615.13 + CR 120.3: a burn spell aimed at the Squire's controller is fully
/// prevented by an INDEPENDENT shield; the Squire's trigger fires once and reads
/// the prevented amount.
///
/// Revert-failing: with `match_unimplemented` restored (or the parser arm
/// removed) the Squire gains 0 counters instead of 4. The amount is asserted
/// CONCRETELY (4, not "nonzero"), so a trigger that fires but misreads the amount
/// also fails.
#[test]
fn non_combat_prevention_puts_that_many_counters_on_the_squire() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let shield = add_shield_enchantment(
        &mut scenario,
        P0,
        "Solitary Confinement",
        SOLITARY_CONFINEMENT_TEXT,
    );
    let bolt = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Prevention Probe",
            true,
            "Prevention Probe deals 4 damage to target player.",
        )
        .id();
    let mut runner = scenario.build();

    // The shield is an independent source — the Squire did not create it.
    runner.cast(shield).resolve();
    assert_eq!(
        counters_on(&runner, squire),
        0,
        "reach-guard: no prevention has happened yet, so the Squire has no counters"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let outcome = runner.cast(bolt).target_player(P0).resolve();

    // Reach-guard: the damage really was PREVENTED, not merely never dealt.
    outcome.assert_life_delta(P0, 0);
    assert_eq!(
        outcome.state().players[P0.0 as usize].life,
        life_before,
        "the shield must have absorbed all 4 damage"
    );

    // CR 615.13: "that many" is the amount of THIS prevention application.
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 4);
}

// ---------------------------------------------------------------------------
// V1 — combat prevention from a PRINTED player shield (charter rows 2 + 5)
//
// Converted from `combat_prevention_currently_blocked_by_object_targeted_aggregate_event`,
// which characterized the defect this row now proves fixed.
// ---------------------------------------------------------------------------

/// CR 615.13 + CR 120.3: a printed, controller-scoped ("dealt to you") shield
/// prevents a whole simultaneous combat-damage batch, and the aggregate event
/// names the PLAYER whose damage was prevented — so the Squire's trigger fires
/// and reads the batch amount.
///
/// This is the 14-card printed `Prevention{All}` / `Player{Controller}` class.
/// Every one of them parses to `Player { player: Controller }`, NOT `Specific`,
/// and the combat aggregate used to re-derive its target from that filter and map
/// only `Specific(PlayerId)` to a player — so it emitted `TargetRef::Object(the
/// enchantment)` and `match_damage_prevented` correctly refused it under CR 120.3
/// (a player-scoped trigger must not fire on object damage). The matcher was
/// right; the event's target was wrong. The recipient is now threaded from the
/// replacement applier, so the filter is never consulted for it.
///
/// BASE BEHAVIOR this inverts (measured): one event `target=Object(field)`,
/// amount 5, and **0** counters.
///
/// Revert-catches: restoring the `damage_target_filter` re-derivation in
/// `fire_combat_prevention_riders`; any fix that resolves `Controller` to the
/// wrong player.
#[test]
fn combat_prevention_from_a_printed_player_shield_puts_that_many_counters() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let shield = add_shield_enchantment(&mut scenario, P0, "Energy Field", ENERGY_FIELD_TEXT);
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    runner.cast(shield).resolve();
    assert_eq!(counters_on(&runner, squire), 0, "reach-guard: pre-combat");

    // Hand the turn to P1 so it can attack.
    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    // CLASS reach-guard: this row must exercise the previously-broken scope, not
    // accidentally land on the one scope that always worked. A printed
    // "dealt to you" shield is Controller-scoped; `Specific` is never printed.
    assert_eq!(
        runner.state().objects[&shield].replacement_definitions[0].damage_target_filter,
        Some(DamageTargetFilter::Player {
            player: DamageTargetPlayerScope::Controller
        }),
        "the printed 'dealt to you' shield is Controller-scoped, not Specific — \
         this is the scope the combat aggregate used to drop"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) =
        run_combat_collecting(&mut runner, P1, &[attacker_a, attacker_b], P0, &[], None);
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: 5 combat damage really WAS prevented. This is what makes the
    // counter assertion below a statement about the TRIGGER rather than about
    // combat never having happened.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "all 5 combat damage must have been prevented by the shield"
    );

    // CR 615.13: ONE application over the CR 510.2 batch, so exactly ONE event —
    // and CR 120.3: it names the PLAYER. Exact list, length included, so a
    // per-source split or a spurious zero-amount entry reddens this row.
    assert_eq!(
        prevented_pairs(&events),
        vec![(TargetRef::Player(P0), 5)],
        "one prevention application over the batch, naming the damaged player"
    );

    assert_eq!(
        counters_on(&runner, squire),
        5,
        "CR 615.13: 'that many' is the amount of THIS application — 2 + 3"
    );
}

// ---------------------------------------------------------------------------
// V2 — one shield, TWO recipients: the discriminating row (charter rows 2 + 5)
// ---------------------------------------------------------------------------

/// CR 615.13 + CR 120.3: one Safe Passage application covers a player leg and a
/// creature leg in the SAME CR 510.2 simultaneous batch. The batch emits one
/// `DamagePrevented` per recipient, each carrying that recipient's own amount,
/// and the player-scoped Squire reads ONLY its own leg.
///
/// This is the row that rejects the tempting "just resolve the shield's
/// controller" fix: that fix emits one `(Player(P0), 5)` and hands the Squire 5
/// counters for damage 3 of which was never headed at a player. The rules-correct
/// answer is 2.
///
/// BASE BEHAVIOR this inverts (measured): ONE event, `target=Object(ObjectId(0))`
/// — a sentinel object that does not exist — amount 5, and **0** counters.
///
/// Revert-catches: the controller-resolving alternative (5 counters); any
/// re-collapse of recipients into one summed event; a lost creature leg; a
/// spurious zero-amount third event.
#[test]
fn one_shield_two_recipients_emits_one_event_each_and_the_squire_reads_only_its_own() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let wall = scenario.add_creature(P0, "Wall", 0, 6).id();
    let passage = add_instant_from_oracle(&mut scenario, P0, "Safe Passage", SAFE_PASSAGE_TEXT);
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[(wall, attacker_b)],
        Some(passage),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // DUAL reach-guard: BOTH legs really were prevented. Without both halves,
    // "2 counters" is also what a lost creature leg looks like.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "the player leg (2 from the unblocked Bear A) must have been prevented"
    );
    assert_eq!(
        runner.state().objects[&wall].damage_marked,
        0,
        "the creature leg (3 from the blocked Bear B) must have been prevented"
    );

    // CR 120.3: exactly two events, one per recipient, each with ITS OWN amount.
    let mut expected = vec![(TargetRef::Player(P0), 2), (TargetRef::Object(wall), 3)];
    expected.sort();
    assert_eq!(
        sorted_prevented_pairs(&events),
        expected,
        "one application, two recipients — the player's 2 and the Wall's 3, not one summed 5"
    );

    // CR 615.13 + CR 120.3: the trigger fires ONCE (one application) and reads
    // the amount prevented FOR ITS OWN RECIPIENT.
    assert_eq!(
        counters_on(&runner, squire),
        2,
        "'damage that would be dealt to YOU' is 2 — explicitly NOT 5, which is \
         what a shield-controller-derived single event would hand it"
    );
}

// ---------------------------------------------------------------------------
// V3 — negative: a shield that prevented nothing FOR THE PLAYER (charter row 2)
// ---------------------------------------------------------------------------

/// CR 120.3: the same Safe Passage, but only a creature is a recipient. There is
/// no player leg, so no player-targeted event and the Squire gains nothing.
///
/// Paired positives live in this same file:
/// `combat_prevention_from_a_printed_player_shield_puts_that_many_counters` and
/// `one_shield_two_recipients_emits_one_event_each_and_the_squire_reads_only_its_own`.
/// The in-test reach-guard is the Wall's `damage_marked == 0`: prevention really
/// did happen, so "0 counters" is a statement about the RECIPIENT and not about
/// nothing having been prevented.
///
/// BASE BEHAVIOR this inverts (measured): one event `target=Object(ObjectId(0))`,
/// amount 3, 0 counters — right answer, wrong reason (a fabricated object).
///
/// Revert-catches: the controller-resolving alternative FABRICATES
/// `(Player(P0), 3)` here and hands the Squire 3 counters for damage never headed
/// at a player.
#[test]
fn a_shield_that_prevented_nothing_for_the_player_emits_no_player_event() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let wall = scenario.add_creature(P0, "Wall", 0, 6).id();
    let passage = add_instant_from_oracle(&mut scenario, P0, "Safe Passage", SAFE_PASSAGE_TEXT);
    let attacker = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker],
        P0,
        &[(wall, attacker)],
        Some(passage),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: prevention DID happen — the 0/6 Wall took none of the 3.
    assert_eq!(
        runner.state().objects[&wall].damage_marked,
        0,
        "the creature leg must have been prevented, so 0 counters below is about \
         the RECIPIENT, not about nothing having happened"
    );

    assert_eq!(
        prevented_pairs(&events),
        vec![(TargetRef::Object(wall), 3)],
        "exactly one event, naming the creature — there is no player leg to name"
    );
    assert_eq!(
        counters_on(&runner, squire),
        0,
        "no damage was headed at the player, so 'damage that would be dealt to \
         you is prevented' never happened"
    );
}

// ---------------------------------------------------------------------------
// V4 — the deleted `Specific` arm's population still fires (charter row 2)
// ---------------------------------------------------------------------------

/// CR 120.3: a RESOLUTION-created player shield. `TargetFilter::Controller` is a
/// context ref, so `untargeted_damage_filter` lowers "prevent all damage that
/// would be dealt to you" to `Player { Specific(P0) }` — the one scope the old
/// re-derivation mapped, and therefore the one population that worked at base.
///
/// This row exists because the fix DELETES that arm. It is the runtime evidence
/// that the ~78-card resolution-created `Specific` / `ParentTarget` population
/// (63 `TargetFilter::Controller` + 15 `ParentTarget` `PreventDamage` effects)
/// still fires in combat with a concrete amount once the recipient is threaded
/// instead of derived.
///
/// BASE BEHAVIOR: identical — this row is green at base by design.
#[test]
fn resolution_created_specific_player_shield_still_fires_in_combat() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let control = add_instant_from_oracle(&mut scenario, P0, "Riot Control", RIOT_CONTROL_TEXT);
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[],
        Some(control),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: the SPELL resolved (it gains 1 life per opponent creature, and
    // P1 controls two) AND no combat damage got through. 22, not 20 and not 17.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        22,
        "Riot Control resolved (+2 life for P1's two creatures) and none of the 5 \
         combat damage was dealt"
    );

    assert_eq!(
        prevented_pairs(&events),
        vec![(TargetRef::Player(P0), 5)],
        "one application over the batch, naming the shielded player"
    );
    assert_eq!(counters_on(&runner, squire), 5, "2 + 3 prevented for P0");
}

// ---------------------------------------------------------------------------
// V5 — the unfiltered shield: no filter to derive a recipient FROM at all
//      (event-shape row for the building block; deliberately NO Squire)
// ---------------------------------------------------------------------------

/// CR 120.3: Fog has no `damage_target_filter` whatsoever — `TargetFilter::Any`
/// lowers to `None` — so there is no scope an arm could ever be added for. The old
/// path fell through to `TargetRef::Object(rid.source)`, and a floating
/// resolution-created shield's `rid.source` is the sentinel `ObjectId(0)`: the
/// event named an object that DOES NOT EXIST. This is the largest broken class
/// (218 resolution `PreventDamage{Any}` shields plus every typed-object shield).
///
/// BASE BEHAVIOR this inverts (measured): ONE event,
/// `target=Object(ObjectId(0))`, amount 9.
///
/// No Squire here on purpose: this row is about the EVENT SHAPE of the shared
/// building block, across three recipients of both kinds, not about the one card
/// that consumes it.
#[test]
fn an_unfiltered_shield_names_every_recipient_not_a_sentinel_object() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let fog = add_instant_from_oracle(&mut scenario, P0, "Fog", FOG_TEXT);
    let attacker_a = scenario.add_creature(P0, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P0, "Bear B", 3, 3).id();
    let blocker = scenario.add_creature(P1, "Grizzly", 4, 4).id();
    let mut runner = scenario.build();

    // Cast pre-combat on P0's own turn: Fog is "this turn", so it is live for the
    // combat-damage step (CR 615.4 — the shield exists before the damage event).
    runner.cast(fog).resolve();

    let life_before = runner.state().players[P1.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P0,
        &[attacker_a, attacker_b],
        P1,
        &[(blocker, attacker_b)],
        None,
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: all 9 really was prevented, across all three recipients.
    assert_eq!(
        runner.state().players[P1.0 as usize].life,
        life_before,
        "the unblocked Bear A's 2 must have been prevented"
    );
    for (creature, label) in [(attacker_b, "Bear B"), (blocker, "Grizzly")] {
        assert_eq!(
            runner.state().objects[&creature].damage_marked,
            0,
            "{label}'s combat damage must have been prevented"
        );
    }

    let mut expected = vec![
        (TargetRef::Player(P1), 2),
        (TargetRef::Object(blocker), 3),
        (TargetRef::Object(attacker_b), 4),
    ];
    expected.sort();
    assert_eq!(
        sorted_prevented_pairs(&events),
        expected,
        "one shield, three recipients — each named, each with its own amount"
    );
    assert!(
        !prevented_pairs(&events)
            .iter()
            .any(|(target, _)| *target == TargetRef::Object(ObjectId(0))),
        "no event may name the ObjectId(0) sentinel — that object does not exist"
    );
}

// ---------------------------------------------------------------------------
// V5(b) — prevention in BOTH combat-damage steps (the hostile case for a latch)
// ---------------------------------------------------------------------------

/// CR 510.4 + CR 702.4b + CR 615.13: first-strike and regular damage are two
/// separate combat-damage steps, each its own CR 510.2 simultaneous batch and
/// therefore its own prevention APPLICATION — so the Squire's trigger fires once
/// per step, each firing reading its own step's amount.
///
/// A 2/2 double-striker and a vanilla 3/3 both go unblocked at the shielded
/// player: the first-strike step prevents 2, the regular step prevents 2 + 3 = 5,
/// and the Squire ends on 2 + 5 = 7.
///
/// This is the Squire-level count the non-combat
/// `two_separate_preventions_each_read_their_own_amount` row cannot supply: a
/// naive once-per-turn latch in the matcher (or a dedup keyed on the turn rather
/// than on the prevention application) fires only for the first step and strands
/// the Squire on 2. The differing per-step amounts (2 then 5, mirroring the
/// 3-then-8 non-combat row) additionally reject an amount bleed: resolving both
/// firings against the first step's amount gives 2 + 2 = 4, against the last
/// gives 5 + 5 = 10.
///
/// The EVENT count per step is pinned at the building-block level by
/// `combat_damage.rs`'s `test_inkshield_double_strike_fires_rider_per_combat_step`;
/// this row pins the SQUIRE-level trigger count and per-step amount provenance.
///
/// Revert-catches: a once-per-turn latch in `match_damage_prevented` (2 counters,
/// caught by the final assertion); any cross-step dedup of the trigger; an
/// amount bleed in either direction (4 or 10 counters).
#[test]
fn double_strike_prevention_fires_once_per_combat_damage_step() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let shield = add_shield_enchantment(&mut scenario, P0, "Energy Field", ENERGY_FIELD_TEXT);
    let striker = {
        let mut b = scenario.add_creature(P1, "Double Striker", 2, 2);
        b.double_strike();
        b.id()
    };
    let bear = scenario.add_creature(P1, "Bear", 3, 3).id();
    let mut runner = scenario.build();

    runner.cast(shield).resolve();
    assert_eq!(counters_on(&runner, squire), 0, "reach-guard: pre-combat");

    // Hand the turn to P1 so it can attack.
    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    // CLASS reach-guard: the same printed Controller-scoped population as V1, so
    // a regression to the filter-derived recipient cannot hide behind the one
    // scope that always worked.
    assert_eq!(
        runner.state().objects[&shield].replacement_definitions[0].damage_target_filter,
        Some(DamageTargetFilter::Player {
            player: DamageTargetPlayerScope::Controller
        }),
        "the printed 'dealt to you' shield is Controller-scoped, not Specific"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(&mut runner, P1, &[striker, bear], P0, &[], None);
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: all 7 really was prevented across the two steps (2 in the
    // first-strike step, 2 + 3 in the regular step). Without this, "7 counters"
    // is also what combat-never-happened looks like.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "all 7 combat damage must have been prevented by the shield"
    );

    // CR 510.4 + CR 615.13: two steps, two batches, two applications — one event
    // per step IN STEP ORDER, each carrying its own step's amount. Exact list,
    // length included, so a collapsed single event or a spurious third entry
    // reddens this row.
    assert_eq!(
        prevented_pairs(&events),
        vec![(TargetRef::Player(P0), 2), (TargetRef::Player(P0), 5)],
        "one prevention application per combat-damage step: 2 (first-strike) then 5 (regular)"
    );

    // CR 615.13: one firing per application, each reading its own step's amount.
    // A once-per-turn latch strands this at 2; a first-amount bleed gives 4; a
    // last-amount bleed gives 10.
    assert_eq!(
        counters_on(&runner, squire),
        7,
        "2 (first-strike step) + 5 (regular step) — explicitly NOT 2, which is \
         what a naive once-per-turn latch in the matcher would leave"
    );
}

// ---------------------------------------------------------------------------
// V6 — the Squire's own canonical play pattern (charter row 2)
// ---------------------------------------------------------------------------

/// CR 615.4 + CR 615.13: Selfless Squire is DESIGNED to be flashed in during
/// combat — its own ETB shield then prevents the batch and its own trigger reads
/// the amount. Single combat, single turn crossing.
///
/// BASE BEHAVIOR: identical — this row is green at base, because the ETB shield is
/// resolution-created from `TargetFilter::Controller` and therefore lowered to
/// `Specific(P0)`, the one scope the old re-derivation mapped. It is here so the
/// card's canonical line cannot regress under a change aimed at the other classes.
#[test]
fn the_squires_own_flashed_shield_fires_its_trigger_in_combat() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = scenario
        .add_creature_to_hand_from_oracle(P0, "Selfless Squire", 1, 1, SELFLESS_SQUIRE_TEXT)
        .with_mana_cost(free_cost())
        .id();
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[],
        Some(squire),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guard: the Squire actually entered AND its ETB shield absorbed the
    // batch. Without the first half, "5 counters" could not even be read.
    assert!(
        runner.state().objects.contains_key(&squire),
        "the flashed Squire must be on the battlefield"
    );
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "the Squire's own ETB shield must have absorbed all 5"
    );

    assert_eq!(
        prevented_pairs(&events),
        vec![(TargetRef::Player(P0), 5)],
        "one application over the batch, naming the shielded player"
    );
    assert_eq!(counters_on(&runner, squire), 5, "2 + 3 prevented for P0");
}

// ---------------------------------------------------------------------------
// V12 — the Squire is ITSELF a recipient in the same batch (charter row 2)
// ---------------------------------------------------------------------------

/// CR 120.3: the per-recipient split newly makes ONE batch carry both an OBJECT
/// recipient and a PLAYER recipient. Only `match_damage_prevented`'s early guard
/// —
///
/// ```ignore
/// if trigger.valid_card.is_none() && trigger.valid_target.is_some() { return false }
/// ```
///
/// — stops the player-scoped Squire from ALSO reading the object leg. This row
/// makes the Squire itself the damaged object, which is the card's natural play
/// (a 1/1 chump-blocking behind Safe Passage), so deleting that guard turns 2
/// counters into 5.
///
/// `a_shield_that_prevented_nothing_for_the_player_emits_no_player_event` does
/// NOT catch that deletion: its creature recipient is a separate 0/6 Wall, so the
/// matcher's fallthrough arm compares `*target_id == source_id` and is false
/// anyway. Only making the trigger's OWN source the damaged object discriminates.
///
/// BASE BEHAVIOR this inverts: one event `target=Object(ObjectId(0))`, amount 5,
/// 0 counters.
#[test]
fn the_squire_itself_blocking_reads_only_the_player_leg() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let passage = add_instant_from_oracle(&mut scenario, P0, "Safe Passage", SAFE_PASSAGE_TEXT);
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[(squire, attacker_b)],
        Some(passage),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // Reach-guards, all three load-bearing.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "the player leg (2 from the unblocked Bear A) must have been prevented"
    );
    assert!(
        runner.state().objects.contains_key(&squire),
        "a 1/1 that survived blocking a 3/3 proves the CREATURE leg really was \
         prevented — if it were not, CR 704.5g would have put it in the graveyard"
    );
    assert_eq!(
        runner.state().objects[&squire].damage_marked,
        0,
        "the Squire took none of the blocked Bear B's 3"
    );

    let mut expected = vec![(TargetRef::Player(P0), 2), (TargetRef::Object(squire), 3)];
    expected.sort();
    assert_eq!(
        sorted_prevented_pairs(&events),
        expected,
        "one application, two recipients — and one of them IS the trigger's source"
    );

    assert_eq!(
        counters_on(&runner, squire),
        2,
        "'damage that would be dealt to YOU' is the player leg only — 2, NOT 5. \
         Deleting the matcher's player-scope guard makes this 5"
    );
}

// ---------------------------------------------------------------------------
// V11 — characterization: reflecting shields keep emitting per damage event
// ---------------------------------------------------------------------------

/// CHARACTERIZATION of a PRE-EXISTING residual that is out of this change's
/// scope, pinned so it cannot move silently.
///
/// A shield whose rider reflects per damage source (Comeuppance) is excluded from
/// `combat_prevention_tally` at BOTH tally writes and reaches the shared
/// per-event emit block in `replacement.rs` instead. So it emits ONE
/// `DamagePrevented` per damage event, and the Squire fires TWICE.
///
/// Under CR 615.13 that OVER-FIRES the trigger: one prevention effect applied to
/// one or more simultaneous damage events is ONE trigger. It is not fixed here
/// because it cannot simply be batched — CR 615.5 binds each reflection to its
/// own damage source ("that much damage to THAT creature" / "THE SOURCE'S
/// controller"), which a batch aggregate destroys. It is outcome-neutral for
/// Selfless Squire only because its effect is linear in the amount (2 + 3 = 5
/// either way); a non-linear consumer would expose it.
///
/// Revert-catches: this change accidentally routing reflecting shields THROUGH
/// the tally, which would collapse these to one event and break the reflections.
#[test]
fn reflecting_shields_still_emit_per_damage_event() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let comeuppance = add_instant_from_oracle(&mut scenario, P0, "Comeuppance", COMEUPPANCE_TEXT);
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[],
        Some(comeuppance),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "reach-guard: all 5 really was prevented"
    );

    let mut expected = vec![(TargetRef::Player(P0), 2), (TargetRef::Player(P0), 3)];
    expected.sort();
    assert_eq!(
        sorted_prevented_pairs(&events),
        expected,
        "CHARACTERIZATION, not an endorsement: a per-source-reflecting shield \
         bypasses the batch tally and emits one event PER DAMAGE EVENT, so the \
         CR 615.13 trigger count is wrong (2 firings where the rules say 1)"
    );
    assert_eq!(
        counters_on(&runner, squire),
        5,
        "two firings of 2 and 3 — outcome-neutral here only because 'that many' \
         is linear in the amount"
    );
}

// ---------------------------------------------------------------------------
// V11-companion — characterization: depletion shields keep emitting per damage event
// ---------------------------------------------------------------------------

/// CHARACTERIZATION of a PRE-EXISTING residual that is out of this change's
/// scope, pinned so it cannot move silently. Sibling to
/// `reflecting_shields_still_emit_per_damage_event` (V11, population (3)); this
/// row covers population (1)/(2) of the three per-event-bypass populations
/// documented in `replacement.rs`: `PreventionAmount::Next(N)` — and, by the
/// same never-sets-`accumulated_in_batch` code path, `AllBut(N)` — never enter
/// the combat batch tally.
///
/// A `Next(7)` player shield facing a simultaneous 2-and-3 batch therefore emits
/// ONE `DamagePrevented` per damage event, and the Squire fires TWICE.
///
/// Under CR 615.13 that OVER-FIRES the trigger: one prevention effect applied to
/// one or more simultaneous damage events is ONE trigger — and CR 615.7's choice
/// ("the shielded player chooses which damage the shield prevents") is about
/// WHICH damage, expressly "the number of events or sources dealing it doesn't
/// matter", so it is not a defense of per-event emission. Like the V11 sibling,
/// it is outcome-neutral for Selfless Squire only because its effect is linear
/// in the amount (2 + 3 = 5 either way); a non-linear consumer would expose it.
///
/// Revert-catches: routing depletion shields THROUGH the tally (the eventual
/// fix) collapses these to one event and reddens this row — at which point the
/// row must be CONVERTED to the fixed expectation, not deleted.
#[test]
fn depletion_shields_still_emit_per_damage_event() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let bulwark = add_instant_from_oracle(
        &mut scenario,
        P0,
        "Depletion Bulwark",
        DEPLETION_BULWARK_TEXT,
    );
    let attacker_a = scenario.add_creature(P1, "Bear A", 2, 2).id();
    let attacker_b = scenario.add_creature(P1, "Bear B", 3, 3).id();
    let mut runner = scenario.build();

    assert_eq!(counters_on(&runner, squire), 0, "reach-guard: pre-combat");

    let crossed = pass_turn_to(&mut runner, P1);
    assert!(
        crossed,
        "stall guard: the turn must actually have passed to P1"
    );

    let life_before = runner.state().players[P0.0 as usize].life;
    let (ran, events) = run_combat_collecting(
        &mut runner,
        P1,
        &[attacker_a, attacker_b],
        P0,
        &[],
        Some(bulwark),
    );
    assert!(ran, "combat reach-guard: the attack must actually have run");

    // CLASS reach-guard: this row must exercise the depletion population, not
    // accidentally land on an `All` shield. A resolution-created instant shield
    // floats in `pending_damage_replacements`, and CR 615.7 depletion arithmetic
    // (7 − 2 − 3) must have left exactly `Next(2)` behind — still installed,
    // since the shield was not used up.
    let depletion = runner
        .state()
        .pending_damage_replacements
        .iter()
        .find(|r| r.shield_kind.is_shield())
        .expect("reach guard: the depletion shield must be installed");
    assert_eq!(
        depletion.shield_kind,
        ShieldKind::Prevention {
            amount: PreventionAmount::Next(2)
        },
        "this row must exercise PreventionAmount::Next, the population that \
         bypasses the tally — 7 minus the 2-and-3 batch leaves Next(2)"
    );

    // Reach-guard: all 5 really was prevented. Without this, "5 counters" is
    // also what combat-never-happened looks like.
    assert_eq!(
        runner.state().players[P0.0 as usize].life,
        life_before,
        "reach-guard: all 5 really was prevented"
    );

    let mut expected = vec![(TargetRef::Player(P0), 2), (TargetRef::Player(P0), 3)];
    expected.sort();
    assert_eq!(
        sorted_prevented_pairs(&events),
        expected,
        "CHARACTERIZATION, not an endorsement: a depletion shield bypasses the \
         batch tally and emits one event PER DAMAGE EVENT, so the CR 615.13 \
         trigger count is wrong (2 firings where the rules say 1)"
    );
    assert_eq!(
        counters_on(&runner, squire),
        5,
        "two firings of 2 and 3 — outcome-neutral here only because 'that many' \
         is linear in the amount"
    );
}

// ---------------------------------------------------------------------------
// T3 — negative: unprevented damage does NOT trigger (charter row 2)
// ---------------------------------------------------------------------------

/// CR 120.3: damage that is actually DEALT is not damage that was prevented.
///
/// The paired positive reach-guard is mandatory and lives in this same test: "0
/// counters" is equally satisfiable by the trigger never being registered at all.
/// Step 2 raises a shield and repeats the same burn, and the Squire must gain
/// counters — proving step 1's zero was a real discrimination.
#[test]
fn unprevented_damage_does_not_trigger_but_prevented_damage_does() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let shield = add_shield_enchantment(
        &mut scenario,
        P0,
        "Solitary Confinement",
        SOLITARY_CONFINEMENT_TEXT,
    );
    let bolt_a = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe A",
            true,
            "Probe A deals 4 damage to target player.",
        )
        .id();
    let bolt_b = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe B",
            true,
            "Probe B deals 4 damage to target player.",
        )
        .id();
    let mut runner = scenario.build();

    // Step 1 — NEGATIVE. No shield yet: the damage lands.
    let outcome = runner.cast(bolt_a).target_player(P0).resolve();
    outcome.assert_life_delta(P0, -4);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 0);

    // Step 2 — PAIRED POSITIVE REACH-GUARD. Same burn, now prevented.
    runner.cast(shield).resolve();
    let outcome = runner.cast(bolt_b).target_player(P0).resolve();
    outcome.assert_life_delta(P0, 0);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 4);
}

// ---------------------------------------------------------------------------
// T4 / T5 — "once" semantics are an EVENT-STREAM property (charter row 4)
// ---------------------------------------------------------------------------

/// CR 603.2c + CR 615.13: two SEPARATE prevention applications in one turn are
/// two trigger events, each reading its OWN amount.
///
/// This is the amount-provenance hostile fixture. Two competing amount
/// authorities live in `quantity.rs`'s cascade (`enclosing_trigger_event_amount`
/// vs `state.last_effect_count`). Preventing 3 and then 5 must yield 3 + 5 = 8
/// total, arriving as 3 first and 8 second. A `last_effect_count` bleed would
/// give 5+5=10 or 3+3=6; a once-per-turn latch would give 3.
#[test]
fn two_separate_preventions_each_read_their_own_amount() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    let shield = add_shield_enchantment(
        &mut scenario,
        P0,
        "Solitary Confinement",
        SOLITARY_CONFINEMENT_TEXT,
    );
    let bolt_a = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe Three",
            true,
            "Probe Three deals 3 damage to target player.",
        )
        .id();
    let bolt_b = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe Five",
            true,
            "Probe Five deals 5 damage to target player.",
        )
        .id();
    let mut runner = scenario.build();

    runner.cast(shield).resolve();

    // First application: 3 prevented → exactly 3 counters.
    let outcome = runner.cast(bolt_a).target_player(P0).resolve();
    outcome.assert_life_delta(P0, 0);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 3);

    // Second application: 5 prevented → 3 + 5 = 8. Not 3 (latched), not 10
    // (last-amount bleed), not 6 (first-amount bleed).
    let outcome = runner.cast(bolt_b).target_player(P0).resolve();
    outcome.assert_life_delta(P0, 0);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 8);
}

// ---------------------------------------------------------------------------
// T6 — recipient scoping end-to-end (charter row 2, CR 120.3)
// ---------------------------------------------------------------------------

/// CR 120.3: "damage that would be dealt to YOU" is the ability's controller.
/// Prevention for a DIFFERENT player must not trigger it.
///
/// Paired positive reach-guard in the same test: the controller's own prevention
/// then fires. Without that pairing, "0 counters" would be satisfiable by a
/// trigger that never registered. Revert-failing on the recipient half: dropping
/// `valid_target = Controller` in the parser (or the `valid_player_matches` call
/// in the matcher) makes step 1 fire and the assertion flips.
#[test]
fn prevention_for_another_player_does_not_trigger_the_squire() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    stock_libraries(&mut scenario);

    let squire = add_squire(&mut scenario, P0);
    // P1's own shield, placed directly on P1's battlefield — the runner casts as
    // the active player (P0), so P1's permanent cannot be routed through `cast`.
    // "Prevent all damage that would be dealt to you" is controller-relative, so
    // on P1's permanent it shields P1.
    let their_shield = scenario
        .add_enchantment_from_oracle(P1, "Solitary Confinement", SOLITARY_CONFINEMENT_TEXT)
        .id();
    let my_shield = add_shield_enchantment(
        &mut scenario,
        P0,
        "Solitary Confinement B",
        SOLITARY_CONFINEMENT_TEXT,
    );
    let bolt_them = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe Them",
            true,
            "Probe Them deals 4 damage to target player.",
        )
        .id();
    let bolt_me = scenario
        .add_spell_to_hand_from_oracle(
            P0,
            "Probe Me",
            true,
            "Probe Me deals 4 damage to target player.",
        )
        .id();
    let mut runner = scenario.build();

    // Reach-guard: P1's shield really is installed, so step 1's prevention will
    // actually occur. Without this, "0 counters" could mean "nothing was ever
    // prevented" rather than "prevention happened for the wrong player".
    assert!(
        !runner.state().objects[&their_shield]
            .replacement_definitions
            .is_empty(),
        "P1's printed shield must be installed on the battlefield"
    );

    // Step 1 — NEGATIVE: damage to P1 is prevented by P1's shield. The Squire's
    // controller is P0, so its trigger must NOT fire.
    let outcome = runner.cast(bolt_them).target_player(P1).resolve();
    outcome.assert_life_delta(P1, 0);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 0);

    // Step 2 — PAIRED POSITIVE REACH-GUARD: the same shape aimed at P0 fires.
    runner.cast(my_shield).resolve();
    let outcome = runner.cast(bolt_me).target_player(P0).resolve();
    outcome.assert_life_delta(P0, 0);
    outcome.assert_counters(squire, CounterType::Plus1Plus1, 4);
}
