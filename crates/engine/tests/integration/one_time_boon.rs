//! Digital-only Alchemy (no CR entry): one-time boons (issue #7495).
//!
//! Runtime behavior: a grant installs a Persistent one-shot `WhenNextEvent`
//! delayed trigger for its holder; the next matching event fires it once and
//! consumes it; "if you have a boon" reads the held list; an intervening-`if`
//! on the granted trigger is checked at fire time and again at resolution
//! (CR 603.4), with a false gate consuming the single occurrence (CR 603.7b).

use engine::game::keywords::object_has_effective_keyword_kind;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    DelayedTriggerCondition, DelayedTriggerLifetime, PerpetualModification, TargetRef,
};
use engine::types::actions::{GameAction, ResolutionOptionalPaymentChoice};
use engine::types::counter::CounterType;
use engine::types::game_state::{StackEntryKind, WaitingFor};
use engine::types::keywords::{Keyword, KeywordKind};
use engine::types::phase::Phase;
use engine::types::triggers::TriggerMode;
use engine::types::zones::Zone;
use engine::types::ObjectId;

use super::rules::AttackTarget;

const LASH: &str = "Illuminating Lash deals 3 damage to any target.\nYou get a one-time boon with \"When you cast a noncreature spell, draw a card.\"";
const BOLT: &str = "Lightning Bolt deals 3 damage to any target.";
const TARGETED_GRANT: &str =
    "Target opponent gets a one-time boon with \"When you cast a noncreature spell, draw a card.\"";
const VALIANT: &str = "Flying\nWhenever Valiant Batrider deals combat damage to a player, that player gets a one-time boon with \"When you cast a noncreature spell, you may pay {1}. If you don't, each opponent draws a card.\"";
const WARLOCK: &str = "Deathtouch\nWhen Underbridge Warlock enters, you get a one-time boon with \"At the beginning of your end step, if three or more creatures died this turn, each opponent loses 5 life and you gain 5 life.\"\nAt the beginning of your end step, if you have a boon, you mill three cards, draw a card, and lose 2 life.";
const PUP: &str = "When Tenacious Pup enters the battlefield, you gain 1 life. You get a one-time boon with \"When you cast a creature spell, that creature enters the battlefield with an additional +1/+1 counter, trample counter, and vigilance counter on it.\"";
const DRAGONBORN: &str = "{2}{R}: This creature gets +1/+0 until end of turn.\nGift of Tiamat — When this creature dies, if its power is greater than 0, note its power. You get a one-time boon with \"When you cast a creature spell, it perpetually gets +X/+0, where X is the noted number.\"";
const MEPHITS: &str = "This sorcery deals 4 damage to target creature or planeswalker. If excess damage was dealt this way, note that excess damage, then you get a one-time boon with \"When you cast a creature spell, it perpetually gets +X/+0, where X is the noted number.\"";
const MOLTEN: &str = "This sorcery deals 4 damage to target creature or planeswalker. If excess damage was dealt this way, note that excess damage, then you get a one-time boon with \"When you cast an instant or sorcery spell, this boon deals damage equal to the noted number to target creature or planeswalker an opponent controls.\"";
const ROTHGA: &str = "Trample\nWhen Rothga, Bonded Engulfer enters, you get a one-time boon with \"When you cast a creature spell, it perpetually gets +X/+X, where X is its power.\"";
// Jaheira, Stirring Harper's boon body on a drivable ETB grantor: Jaheira
// herself grants on specialize, which has no scenario driver, so the runtime
// pins the shared inner shape while `boon_jaheira_stirring_harper` pins her
// text to that same shape.
const JAHEIRA_INNER_GRANT: &str = "When ~ enters, you get a one-time boon with \"When you cast a creature spell, it perpetually gets +1/+0 and gains haste.\"";
const DUNBARROW: &str = "When Dunbarrow Revivalist enters, you get a one-time boon with \"When one or more creatures enter under your control, create a Wicked Role token attached to one of them.\"";
const RAISE_ALARM: &str = "Create two 1/1 white Soldier creature tokens.";
const DOUBLING_SEASON: &str = "If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.\nIf an effect would put one or more counters on a permanent you control, it puts twice that many of those counters on that permanent instead.";
const HALVING_SEASON: &str = "If an opponent would create one or more tokens, they create half that many of each of those kinds of tokens instead, rounded down.\nIf an opponent would put one or more counters on a permanent or player, they put half that many of each of those kinds of counters on that permanent or player instead, rounded down.";

/// Resolve the stack fully, ordering simultaneous triggers in listed order.
/// Stops on an empty stack at priority — never passes into the next phase.
fn drain_stack(runner: &mut GameRunner) {
    for _ in 0..100 {
        if runner.state().stack.is_empty()
            && !matches!(runner.state().waiting_for, WaitingFor::OrderTriggers { .. })
        {
            break;
        }
        match &runner.state().waiting_for {
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order simultaneous triggers");
            }
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            other => panic!("unexpected prompt while draining stack: {other:?}"),
        }
    }
    assert!(
        runner.state().stack.is_empty(),
        "stack must drain: {:?}",
        runner.state().stack
    );
}

/// Advance to the end step, declaring no attackers if combat prompts.
/// Precondition: call at or after combat (these tests set up at
/// `PostCombatMain`) — crossing combat from a main phase with no legal
/// attackers lets the turn driver skip the declare step entirely.
fn advance_to_end_step_peaceful(runner: &mut GameRunner) {
    if matches!(
        runner.state().waiting_for,
        WaitingFor::DeclareAttackers { .. }
    ) {
        runner
            .act(GameAction::DeclareAttackers {
                attacks: vec![],
                bands: vec![],
            })
            .expect("declare no attackers to cross combat");
    }
    runner.advance_to_end_step();
}

/// Held boons and their controllers.
fn held_boons(runner: &GameRunner) -> Vec<engine::types::player::PlayerId> {
    runner
        .state()
        .delayed_triggers
        .iter()
        .filter(|dt| dt.kind.is_boon())
        .map(|dt| dt.controller)
        .collect()
}

/// The per-grant noted-number capture each held boon snapshotted at install
/// time, in install order. A boon reads its own capture when its body
/// resolves; a `None` capture (its resolution noted nothing) falls back to
/// the live global.
fn held_boon_captures(runner: &GameRunner) -> Vec<Option<i32>> {
    runner
        .state()
        .delayed_triggers
        .iter()
        .filter(|dt| dt.kind.is_boon())
        .map(|dt| dt.ability.context.boon_captured_noted_number)
        .collect()
}

/// The frozen perpetual P/T records on an object: dynamic deltas evaluate
/// once at application time and freeze to `Fixed`, so only `Fixed` exprs
/// may persist — never a live expression.
fn frozen_perpetual_pt(runner: &GameRunner, id: ObjectId) -> Vec<(i32, i32)> {
    use engine::types::ability::QuantityExpr;
    runner.state().objects[&id]
        .perpetual_mods
        .iter()
        .filter_map(|modification| match modification {
            PerpetualModification::ModifyPowerToughness {
                power: QuantityExpr::Fixed { value: power_delta },
                toughness:
                    QuantityExpr::Fixed {
                        value: toughness_delta,
                    },
                ..
            } => Some((*power_delta, *toughness_delta)),
            _ => None,
        })
        .collect()
}

fn assert_no_dynamic_perpetual(runner: &GameRunner, id: ObjectId) {
    use engine::types::ability::QuantityExpr;
    assert!(
        !runner.state().objects[&id]
            .perpetual_mods
            .iter()
            .any(|m| matches!(
                m,
                PerpetualModification::ModifyPowerToughness { power, toughness, .. }
                if !matches!(power, QuantityExpr::Fixed { .. })
                    || !matches!(toughness, QuantityExpr::Fixed { .. })
            )),
        "dynamic perpetuals must freeze at application, never persist live: {:?}",
        runner.state().objects[&id].perpetual_mods
    );
}

#[test]
fn lash_installs_persistent_one_shot_boon() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let lash = scenario
        .add_spell_to_hand_from_oracle(P0, "Illuminating Lash", false, LASH)
        .id();
    let mut runner = scenario.build();
    runner.cast(lash).target_player(P1).resolve();

    assert_eq!(runner.state().players[1].life, 17);
    let boons: Vec<_> = runner
        .state()
        .delayed_triggers
        .iter()
        .filter(|dt| dt.kind.is_boon())
        .collect();
    assert_eq!(boons.len(), 1, "one boon must be held");
    let boon = boons[0];
    assert_eq!(boon.controller, P0);
    assert!(boon.one_shot);
    let DelayedTriggerCondition::WhenNextEvent {
        trigger, lifetime, ..
    } = &boon.condition
    else {
        panic!("boon must be a WhenNextEvent, got {:?}", boon.condition);
    };
    assert_eq!(*lifetime, DelayedTriggerLifetime::Persistent);
    assert_eq!(trigger.mode, TriggerMode::SpellCast);
}

#[test]
fn lash_boon_draws_on_first_cast_only() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["Draw A", "Draw B", "Draw C"]);
    let lash = scenario
        .add_spell_to_hand_from_oracle(P0, "Illuminating Lash", false, LASH)
        .id();
    let bolt_one = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt One", true, BOLT)
        .id();
    let bolt_two = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt Two", true, BOLT)
        .id();
    let mut runner = scenario.build();
    runner.cast(lash).target_player(P1).resolve();
    assert_eq!(held_boons(&runner), vec![P0]);

    // First noncreature cast fires the boon: cast (-1) then draw (+1).
    let hand_before = runner.state().players[0].hand.len();
    runner.cast(bolt_one).target_player(P1).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].hand.len(), hand_before);
    assert!(
        held_boons(&runner).is_empty(),
        "the fired boon must be consumed"
    );

    // Second cast finds no boon: no draw.
    let hand_before = runner.state().players[0].hand.len();
    runner.cast(bolt_two).target_player(P1).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].hand.len(), hand_before - 1);
    assert_eq!(runner.state().players[1].life, 11);
}

#[test]
fn targeted_grant_fires_for_holder_only() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["P0 Draw"]);
    scenario.with_library_top(P1, &["P1 Draw"]);
    let grant = scenario
        .add_spell_to_hand_from_oracle(P0, "Targeted Grant", false, TARGETED_GRANT)
        .id();
    let bolt_p0 = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt P0", true, BOLT)
        .id();
    let bolt_p1 = scenario
        .add_spell_to_hand_from_oracle(P1, "Bolt P1", true, BOLT)
        .id();
    let mut runner = scenario.build();
    runner.cast(grant).target_player(P1).resolve();
    assert_eq!(held_boons(&runner), vec![P1]);

    // The creator's own cast does not fire the opponent's boon.
    let hand_before = runner.state().players[0].hand.len();
    runner.cast(bolt_p0).target_player(P1).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].hand.len(), hand_before - 1);
    assert_eq!(held_boons(&runner), vec![P1]);

    // The holder's cast fires it: cast (-1) then draw (+1), then consumed.
    runner
        .act(GameAction::PassPriority)
        .expect("pass priority to P1");
    let hand_before = runner.state().players[1].hand.len();
    runner.cast(bolt_p1).target_player(P0).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[1].hand.len(), hand_before);
    assert!(held_boons(&runner).is_empty());
    // The draw went to the holder, not the creator.
    assert_eq!(runner.state().players[1].library.len(), 0);
    assert_eq!(runner.state().players[0].library.len(), 1);
}

#[test]
fn valiant_batrider_grants_boon_to_damaged_player() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(P0, &["P0 Draw"]);
    let valiant = scenario
        .add_creature_from_oracle(P0, "Valiant Batrider", 2, 2, VALIANT)
        .with_subtypes(vec!["Human", "Knight"])
        .id();
    let bolt_p1 = scenario
        .add_spell_to_hand_from_oracle(P1, "Bolt P1", true, BOLT)
        .id();
    let mut runner = scenario.build();

    runner.advance_to_combat();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![(valiant, AttackTarget::Player(P1))],
            bands: vec![],
        })
        .expect("declare attack");
    for _ in 0..40 {
        match &runner.state().waiting_for {
            WaitingFor::DeclareBlockers { .. } => {
                runner
                    .act(GameAction::DeclareBlockers {
                        assignments: vec![],
                    })
                    .expect("no blocks");
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            _ if runner.state().phase == Phase::PostCombatMain => break,
            _ => runner.pass_both_players(),
        }
    }

    // Combat damage resolved: P1 was dealt 2 and now holds the boon.
    assert_eq!(runner.state().players[1].life, 18);
    assert_eq!(held_boons(&runner), vec![P1]);

    // P1's noncreature cast fires it; declining {1} makes each opponent draw.
    runner
        .act(GameAction::PassPriority)
        .expect("pass priority to P1");
    let hand_before = runner.state().players[0].hand.len();
    runner.cast(bolt_p1).target_player(P0).commit();
    for _ in 0..100 {
        if runner.state().stack.is_empty()
            && !matches!(
                runner.state().waiting_for,
                WaitingFor::OrderTriggers { .. }
                    | WaitingFor::ResolutionOptionalPaymentChoice { .. }
                    | WaitingFor::OptionalEffectChoice { .. }
            )
        {
            break;
        }
        match &runner.state().waiting_for {
            WaitingFor::OptionalEffectChoice { .. } => {
                runner
                    .act(GameAction::DecideOptionalEffect { accept: false })
                    .expect("decline the {1} payment");
            }
            WaitingFor::ResolutionOptionalPaymentChoice { .. } => {
                runner
                    .act(GameAction::ChooseResolutionOptionalPaymentBranch {
                        choice: ResolutionOptionalPaymentChoice::Decline,
                    })
                    .expect("decline the {1} payment");
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert_eq!(
        runner.state().players[0].hand.len(),
        hand_before + 1,
        "P0 draws from the declined payment"
    );
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn warlock_false_gate_consumes_boon_and_fizzles_have_a_boon() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PostCombatMain);
    scenario.with_library_top(P0, &["A", "B", "C", "D"]);
    scenario.with_library_top(P1, &["P1 A"]);
    let warlock = scenario
        .add_creature_to_hand_from_oracle(P0, "Underbridge Warlock", 2, 4, WARLOCK)
        .id();
    let mut runner = scenario.build();
    runner.cast(warlock).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    // End step with nothing died: the boon's gate is false, so it is
    // discarded without firing — and the have-a-boon trigger, true at fire
    // time, finds no boon at its CR 603.4 resolution recheck and does nothing.
    let hand_before = runner.state().players[0].hand.len();
    let grave_before = runner.state().players[0].graveyard.len();
    advance_to_end_step_peaceful(&mut runner);
    drain_stack(&mut runner);
    assert!(held_boons(&runner).is_empty());
    assert_eq!(runner.state().players[0].hand.len(), hand_before);
    assert_eq!(runner.state().players[0].graveyard.len(), grave_before);
    assert_eq!(runner.state().players[0].life, 20);
    assert_eq!(runner.state().players[1].life, 20);
}

#[test]
fn warlock_have_a_boon_resolves_while_lash_boon_held() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PostCombatMain);
    scenario.with_library_top(P0, &["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"]);
    scenario.with_library_top(P1, &["P1 A"]);
    let warlock = scenario
        .add_creature_to_hand_from_oracle(P0, "Underbridge Warlock", 2, 4, WARLOCK)
        .id();
    let lash = scenario
        .add_spell_to_hand_from_oracle(P0, "Illuminating Lash", false, LASH)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let mut runner = scenario.build();

    // The Warlock's own end-step boon is held, and Lash grants a second boon
    // whose event (a cast) cannot occur during the end step — so have-a-boon
    // is true at fire time AND still true at its CR 603.4 resolution recheck
    // even after the end-step boon's false gate discards it.
    runner.cast(warlock).resolve();
    drain_stack(&mut runner);
    runner.cast(lash).target_player(P1).resolve();
    assert_eq!(held_boons(&runner).len(), 2);
    assert_eq!(runner.state().players[1].life, 17);
    let hand_before = runner.state().players[0].hand.len();
    let grave_before = runner.state().players[0].graveyard.len();
    advance_to_end_step_peaceful(&mut runner);
    drain_stack(&mut runner);
    assert_eq!(
        runner.state().players[0].graveyard.len(),
        grave_before + 3,
        "mill three"
    );
    assert_eq!(
        runner.state().players[0].hand.len(),
        hand_before + 1,
        "draw a card"
    );
    assert_eq!(runner.state().players[0].life, 18);
    assert_eq!(
        held_boons(&runner),
        vec![P0],
        "the Lash boon survives the end step"
    );

    // The surviving boon still fires on the next noncreature cast.
    let hand_before = runner.state().players[0].hand.len();
    runner.cast(bolt).target_player(P1).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].hand.len(), hand_before);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn pup_boon_adds_counters_to_next_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let pup = scenario
        .add_creature_to_hand_from_oracle(P0, "Tenacious Pup", 2, 2, PUP)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();
    runner.cast(pup).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].life, 21);
    assert_eq!(held_boons(&runner), vec![P0]);

    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = runner.state().objects[&bear].clone();
    assert_eq!(
        entered.counters.get(&CounterType::Plus1Plus1),
        Some(&1),
        "enters with a +1/+1 counter: {:?}",
        entered.counters
    );
    assert_eq!(
        entered
            .counters
            .get(&CounterType::Keyword(KeywordKind::Trample)),
        Some(&1),
        "enters with a trample counter: {:?}",
        entered.counters
    );
    assert_eq!(
        entered
            .counters
            .get(&CounterType::Keyword(KeywordKind::Vigilance)),
        Some(&1),
        "enters with a vigilance counter: {:?}",
        entered.counters
    );
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn dragonborn_dies_notes_power_and_pumps_next_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dragonborn = scenario
        .add_creature_from_oracle(P0, "Dragonborn Immolator", 3, 3, DRAGONBORN)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // Bolt kills the 3/3: the dies trigger notes 3 and grants the boon.
    runner.cast(bolt).target_object(dragonborn).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(3));
    assert_eq!(held_boons(&runner), vec![P0]);

    // The next creature spell perpetually gets +3/+0, then the boon is spent.
    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(5), Some(2)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(3, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn dragonborn_zero_power_never_triggers() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dragonborn = scenario
        .add_creature_from_oracle(P0, "Dragonborn Immolator", 3, 3, DRAGONBORN)
        .id();
    // Same oracle text, distinct name (no name self-reference in the text,
    // so the rename is behavior-preserving and side-steps the legend rule).
    let dragonborn_zero = scenario
        .add_creature_from_oracle(P0, "Dragonborn Whelp", 0, 3, DRAGONBORN)
        .id();
    let bolt_one = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt One", true, BOLT)
        .id();
    let bolt_two = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt Two", true, BOLT)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // The 3/3 dies first: notes 3 and grants a boon, leaving a stale
    // nonzero note behind for the sting below.
    runner.cast(bolt_one).target_object(dragonborn).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(3));
    assert_eq!(held_boons(&runner), vec![P0]);

    // Power 0 fails the CR 603.4 intervening-if, so the trigger never
    // fires at all: no note, no grant — even with the stale note 3 live.
    runner
        .cast(bolt_two)
        .target_object(dragonborn_zero)
        .resolve();
    drain_stack(&mut runner);
    assert_eq!(
        runner.state().players[0].noted_number,
        Some(3),
        "a gated-off trigger notes nothing"
    );
    assert_eq!(
        held_boons(&runner),
        vec![P0],
        "a gated-off trigger grants nothing"
    );
    assert_eq!(held_boon_captures(&runner), vec![Some(3)]);

    // Only the first boon fires on the bear: 2 + 3.
    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(5), Some(2)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(3, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn mephits_excess_notes_and_pumps_next_creature() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let wall = scenario.add_creature_from_oracle(P1, "Wall", 0, 2, "").id();
    let mephits = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's Enthusiasm", false, MEPHITS)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // 4 damage to a 2-toughness creature: excess 2 is noted, boon granted.
    runner.cast(mephits).target_object(wall).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(2));
    assert_eq!(held_boons(&runner), vec![P0]);

    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(4), Some(2)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(2, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn mephits_no_excess_notes_nothing_and_grants_nothing() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let giant = scenario
        .add_creature_from_oracle(P1, "Giant", 4, 5, "")
        .id();
    let mephits = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's Enthusiasm", false, MEPHITS)
        .id();
    let mut runner = scenario.build();

    // 4 damage to a 5-toughness creature: no excess, so both gated legs skip.
    runner.cast(mephits).target_object(giant).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, None);
    assert!(held_boons(&runner).is_empty(), "no excess means no grant");
    assert_eq!(runner.state().players[1].life, 20);
    assert_eq!(
        runner.state().objects[&giant].zone,
        Zone::Battlefield,
        "the giant survives at 4 marked damage"
    );
}

#[test]
fn molten_boon_deals_noted_damage() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let wall_one = scenario
        .add_creature_from_oracle(P1, "Wall One", 0, 2, "")
        .id();
    let wall_two = scenario
        .add_creature_from_oracle(P1, "Wall Two", 0, 2, "")
        .id();
    let molten = scenario
        .add_spell_to_hand_from_oracle(P0, "Molten Impact", false, MOLTEN)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let mut runner = scenario.build();

    runner.cast(molten).target_object(wall_one).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(2));
    assert_eq!(held_boons(&runner), vec![P0]);

    // The Bolt cast fires the boon; the boon trigger needs its own target.
    runner.cast(bolt).target_player(P1).commit();
    for _ in 0..100 {
        if runner.state().stack.is_empty()
            && !matches!(
                runner.state().waiting_for,
                WaitingFor::OrderTriggers { .. }
                    | WaitingFor::TriggerTargetSelection { .. }
                    | WaitingFor::TargetSelection { .. }
            )
        {
            break;
        }
        match &runner.state().waiting_for {
            WaitingFor::TriggerTargetSelection { .. } | WaitingFor::TargetSelection { .. } => {
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(wall_two)),
                    })
                    .expect("choose the boon target");
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    // The boon dealt the noted 2 (lethal to Wall Two); Bolt dealt 3 to P1.
    assert_eq!(runner.state().objects[&wall_two].zone, Zone::Graveyard);
    assert_eq!(runner.state().players[1].life, 17);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn rothga_perpetual_reads_spell_power_not_granter() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let rothga = scenario
        .add_creature_to_hand_from_oracle(P0, "Rothga, Bonded Engulfer", 4, 4, ROTHGA)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    runner.cast(rothga).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    // X is the 2-power spell's power — Rothga's own 4 must not leak in.
    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(4), Some(4)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(2, 2)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn jaheira_boon_grants_perpetual_pt_and_haste_to_cast_creature() {
    use engine::types::ability::QuantityExpr;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let grantor = scenario
        .add_creature_to_hand_from_oracle(P0, "Jaheira Proxy", 2, 2, JAHEIRA_INNER_GRANT)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    runner.cast(grantor).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(3), Some(2)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(1, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    // One combined record — the +1/+0 and the haste ride a single
    // modification, not sibling records.
    let combined = runner.state().objects[&bear]
        .perpetual_mods
        .iter()
        .filter(|m| {
            matches!(
                m,
                PerpetualModification::ModifyPowerToughness {
                    power: QuantityExpr::Fixed { value: 1 },
                    toughness: QuantityExpr::Fixed { value: 0 },
                    keywords,
                } if keywords.iter().any(|k| matches!(k, Keyword::Haste))
            )
        })
        .count();
    assert_eq!(combined, 1);
    assert!(
        object_has_effective_keyword_kind(runner.state(), bear, KeywordKind::Haste),
        "the cast creature must have perpetual haste"
    );
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn sequential_notes_grant_independent_boons() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let wall_a = scenario
        .add_creature_from_oracle(P1, "Wall A", 0, 3, "")
        .id();
    let wall_b = scenario
        .add_creature_from_oracle(P1, "Wall B", 0, 1, "")
        .id();
    let mephits_one = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's One", false, MEPHITS)
        .id();
    let mephits_two = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's Two", false, MEPHITS)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    runner.cast(mephits_one).target_object(wall_a).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(1));
    runner.cast(mephits_two).target_object(wall_b).resolve();
    drain_stack(&mut runner);
    assert_eq!(
        runner.state().players[0].noted_number,
        Some(3),
        "the live global still overwrites"
    );
    assert_eq!(held_boons(&runner).len(), 2);
    // Each grant captured what its own resolution noted — the second note
    // must not leak into the first boon.
    assert_eq!(held_boon_captures(&runner), vec![Some(1), Some(3)]);

    // Both boons fire on the one cast, each pumping its own captured note
    // (2 + 1 + 3). Two simultaneous triggers need manual driving: the cast
    // driver's commit loop does not order triggers.
    let bear_card = runner.state().objects[&bear].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: bear,
            card_id: bear_card,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Bear");
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(6), Some(2)));
    let mut frozen = frozen_perpetual_pt(&runner, bear);
    frozen.sort_unstable();
    assert_eq!(frozen, vec![(1, 0), (3, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

/// Resolution-local note provenance: a grant whose own resolution noted
/// nothing captures `None` even with a stale live note in place, so a
/// later note reaches its live fallback. Synthetic grantor (no printed
/// note-reading grant lacks a note leg); the inner mirrors Mephit's.
const NOTELESS_GRANT: &str = "When ~ enters, you get a one-time boon with \"When you cast a creature spell, it perpetually gets +X/+0, where X is the noted number.\"";

#[test]
fn noteless_grant_ignores_stale_note_and_reads_live() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let wall_a = scenario
        .add_creature_from_oracle(P1, "Wall A", 0, 3, "")
        .id();
    let wall_b = scenario
        .add_creature_from_oracle(P1, "Wall B", 0, 1, "")
        .id();
    let mephits_one = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's One", false, MEPHITS)
        .id();
    let mephits_two = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's Two", false, MEPHITS)
        .id();
    let proxy = scenario
        .add_creature_to_hand_from_oracle(P0, "Noteless Proxy", 2, 2, NOTELESS_GRANT)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // Pre-existing note 1 in place, with its own grant capturing 1.
    runner.cast(mephits_one).target_object(wall_a).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(1));

    // Casting the proxy fires boonA first (positive control: it captured its
    // own 1). The noteless grant then resolves with the stale note live: it
    // must capture `None`, not 1. Manual cast: the SpellCast driver does not
    // order the two triggers (boonA's fire + the proxy's own ETB).
    let proxy_card = runner.state().objects[&proxy].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: proxy,
            card_id: proxy_card,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Proxy");
    drain_stack(&mut runner);
    let proxied = &runner.state().objects[&proxy];
    assert_eq!((proxied.power, proxied.toughness), (Some(3), Some(2)));
    assert_eq!(held_boon_captures(&runner), vec![None]);

    // A later note overwrites the live global (and grants its own boon).
    runner.cast(mephits_two).target_object(wall_b).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(3));
    assert_eq!(held_boon_captures(&runner), vec![None, Some(3)]);

    // One cast fires both: the noteless boon reads the live 3, not the
    // stale 1 (2 + 3 + 3). Manual cast again for the two simultaneous fires.
    let bear_card = runner.state().objects[&bear].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: bear,
            card_id: bear_card,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Bear");
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(8), Some(2)));
    let mut frozen = frozen_perpetual_pt(&runner, bear);
    frozen.sort_unstable();
    assert_eq!(frozen, vec![(3, 0), (3, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}

/// Every Role token in the game, in every zone on purpose: a token that
/// reached the battlefield and was swept looks identical to one never
/// created under a battlefield-only query.
fn role_tokens(runner: &GameRunner) -> Vec<ObjectId> {
    runner
        .state()
        .objects
        .values()
        .filter(|object| object.is_token && object.card_types.subtypes.iter().any(|s| s == "Role"))
        .map(|object| object.id)
        .collect()
}

#[test]
fn dunbarrow_two_entrants_offer_host_choice() {
    use engine::game::game_object::AttachTarget;
    use engine::types::ability::TargetRef;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dunbarrow = scenario
        .add_creature_to_hand_from_oracle(P0, "Dunbarrow Revivalist", 3, 3, DUNBARROW)
        .id();
    let alarm = scenario
        .add_spell_to_hand_from_oracle(P0, "Raise Alarm", false, RAISE_ALARM)
        .id();
    let mut runner = scenario.build();

    runner.cast(dunbarrow).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    // Two soldiers enter in one batch; the boon fires once. Manual driving:
    // the cast driver's commit loop does not answer the host choice.
    runner.cast(alarm).commit();
    let mut answered = false;
    for _ in 0..200 {
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            WaitingFor::ChooseTokenHost { legal_targets, .. } => {
                assert_eq!(
                    legal_targets.len(),
                    2,
                    "both entrants must be offered, got {legal_targets:?}"
                );
                // Choose deterministically: the greater soldier id.
                let mut soldiers: Vec<ObjectId> = legal_targets
                    .iter()
                    .filter_map(|t| match t {
                        TargetRef::Object(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                soldiers.sort();
                let chosen = soldiers[1];
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(chosen)),
                    })
                    .expect("choose host");
                answered = true;
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(answered, "the host choice must have been offered");
    drain_stack(&mut runner);

    // The Role entered attached to the CHOSEN soldier — not the first.
    let roles = role_tokens(&runner);
    assert_eq!(roles.len(), 1, "one Role must be created");
    let role = &runner.state().objects[&roles[0]];
    assert_eq!(role.zone, Zone::Battlefield);
    let mut soldiers: Vec<ObjectId> = runner
        .state()
        .objects
        .values()
        .filter(|o| {
            o.is_token && o.controller == P0 && o.card_types.subtypes.iter().any(|s| s == "Soldier")
        })
        .map(|o| o.id)
        .collect();
    soldiers.sort();
    assert_eq!(soldiers.len(), 2);
    assert_eq!(
        role.attached_to,
        Some(AttachTarget::Object(soldiers[1])),
        "the Role must attach to the chosen entrant"
    );
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn dunbarrow_single_entrant_attaches_without_prompt() {
    use engine::game::game_object::AttachTarget;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dunbarrow = scenario
        .add_creature_to_hand_from_oracle(P0, "Dunbarrow Revivalist", 3, 3, DUNBARROW)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    runner.cast(dunbarrow).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    // One entrant: auto-bound, no prompt (`drain_stack` panics on any
    // unexpected prompt, proving none was offered).
    runner.cast(bear).resolve();
    drain_stack(&mut runner);

    let roles = role_tokens(&runner);
    assert_eq!(roles.len(), 1, "one Role must be created");
    let role = &runner.state().objects[&roles[0]];
    assert_eq!(role.zone, Zone::Battlefield);
    assert_eq!(
        role.attached_to,
        Some(AttachTarget::Object(bear)),
        "the Role must attach to the lone entrant"
    );
    assert!(held_boons(&runner).is_empty());
}

/// CR 616.1: one Dunbarrow batch under the holder's Doubling Season and the
/// opponent's Halving Season, applying `first` first. Integer halving does
/// not commute with doubling, so double-then-halve yields one Role while
/// halve-then-double yields zero — the host-choice resume must preserve the
/// replacement pause instead of settling to Priority and dropping it.
fn dunbarrow_season_order(first: &str, expect_roles: usize) {
    use engine::game::game_object::AttachTarget;
    use engine::types::ability::TargetRef;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_enchantment_from_oracle(P0, "Doubling Season", DOUBLING_SEASON);
    scenario.add_enchantment_from_oracle(P1, "Halving Season", HALVING_SEASON);
    let dunbarrow = scenario
        .add_creature_to_hand_from_oracle(P0, "Dunbarrow Revivalist", 3, 3, DUNBARROW)
        .id();
    let alarm = scenario
        .add_spell_to_hand_from_oracle(P0, "Raise Alarm", false, RAISE_ALARM)
        .id();
    let mut runner = scenario.build();

    runner.cast(dunbarrow).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    let other = if first == "twice" { "half" } else { "twice" };
    let mut host_answered = false;
    let mut chosen_host = None;
    let mut role_prompts = 0;
    runner.cast(alarm).commit();
    for _ in 0..200 {
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            WaitingFor::ChooseTokenHost { legal_targets, .. } => {
                assert!(
                    legal_targets.len() >= 2,
                    "the batch must offer a host choice, got {legal_targets:?}"
                );
                let mut soldiers: Vec<ObjectId> = legal_targets
                    .iter()
                    .filter_map(|t| match t {
                        TargetRef::Object(id) => Some(*id),
                        _ => None,
                    })
                    .collect();
                soldiers.sort();
                let chosen = soldiers[soldiers.len() - 1];
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(chosen)),
                    })
                    .expect("choose host");
                host_answered = true;
                chosen_host = Some(chosen);
            }
            WaitingFor::ReplacementChoice { candidates, .. } => {
                // Pre-host prompts belong to the soldiers (either order lands
                // on two: 2*2/2 == 2 and 2/2*2 == 2); post-host prompts are
                // the Role's ordering and follow `first`.
                let want = if !host_answered {
                    None
                } else {
                    role_prompts += 1;
                    Some(if role_prompts == 1 { first } else { other })
                };
                let index = match want {
                    None => 0,
                    Some(want) => candidates
                        .iter()
                        .position(|c| c.description.contains(want))
                        .unwrap_or_else(|| {
                            panic!(
                                "expected a {want} candidate, got {:?}",
                                candidates
                                    .iter()
                                    .map(|c| c.description.as_str())
                                    .collect::<Vec<_>>()
                            )
                        }),
                };
                runner
                    .act(GameAction::ChooseReplacement { index })
                    .expect("choose replacement");
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(host_answered, "the host choice must have been offered");
    assert!(
        role_prompts >= 1,
        "answering the host must surface the Role's replacement pause"
    );
    drain_stack(&mut runner);

    let roles = role_tokens(&runner);
    assert_eq!(
        roles.len(),
        expect_roles,
        "ordering {first} first must create exactly {expect_roles} Role(s)"
    );
    if expect_roles == 1 {
        let role = &runner.state().objects[&roles[0]];
        assert_eq!(role.zone, Zone::Battlefield);
        assert_eq!(
            role.attached_to,
            Some(AttachTarget::Object(chosen_host.expect("host"))),
            "the Role must attach to the chosen entrant"
        );
    }
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn dunbarrow_double_then_halve_creates_one_role() {
    dunbarrow_season_order("twice", 1);
}

#[test]
fn dunbarrow_halve_then_double_creates_no_role() {
    dunbarrow_season_order("half", 0);
}

/// CR 608.2c: a token-host instruction with a following instruction parks its
/// tail while the host choice is pending. Synthetic inner (no printed boon
/// pairs "one of them" with a tail): the life gain must NOT have applied
/// while the prompt is open, and must apply exactly once after it resolves.
const DUNBARROW_TAIL: &str = "When ~ enters, you get a one-time boon with \"When one or more creatures enter under your control, create a Wicked Role token attached to one of them. You gain 2 life.\"";

#[test]
fn dunbarrow_host_choice_parks_following_instruction() {
    use engine::types::ability::TargetRef;

    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let grantor = scenario
        .add_creature_to_hand_from_oracle(P0, "Dunbarrow Proxy", 3, 3, DUNBARROW_TAIL)
        .id();
    let alarm = scenario
        .add_spell_to_hand_from_oracle(P0, "Raise Alarm", false, RAISE_ALARM)
        .id();
    let mut runner = scenario.build();

    runner.cast(grantor).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);
    assert_eq!(runner.state().players[0].life, 20);

    runner.cast(alarm).commit();
    let mut answered = false;
    for _ in 0..200 {
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                runner
                    .act(GameAction::OrderTriggers { order })
                    .expect("order triggers");
            }
            WaitingFor::ChooseTokenHost { legal_targets, .. } => {
                assert_eq!(
                    runner.state().players[0].life,
                    20,
                    "the tail must not run before the host is chosen"
                );
                let chosen = legal_targets
                    .iter()
                    .filter_map(|t| match t {
                        TargetRef::Object(id) => Some(*id),
                        _ => None,
                    })
                    .max()
                    .expect("legal host");
                runner
                    .act(GameAction::ChooseTarget {
                        target: Some(TargetRef::Object(chosen)),
                    })
                    .expect("choose host");
                answered = true;
            }
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(answered, "the host choice must have been offered");
    drain_stack(&mut runner);
    assert_eq!(
        runner.state().players[0].life,
        22,
        "the parked tail must run exactly once after the host resolves"
    );
    assert_eq!(role_tokens(&runner).len(), 1);
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn dunbarrow_departed_entrant_creates_no_token() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dunbarrow = scenario
        .add_creature_to_hand_from_oracle(P0, "Dunbarrow Revivalist", 3, 3, DUNBARROW)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let mut runner = scenario.build();

    runner.cast(dunbarrow).resolve();
    drain_stack(&mut runner);
    assert_eq!(held_boons(&runner), vec![P0]);

    // The bear enters (firing the boon), then dies in response to the
    // trigger: at resolution no stamped entrant is legal, so CR 303.4i
    // denies the Aura token entirely.
    runner.cast(bear).commit();
    let mut bolted = false;
    for _ in 0..200 {
        if !bolted
            && matches!(runner.state().waiting_for, WaitingFor::Priority { .. })
            && !runner.state().stack.is_empty()
            && runner.state().objects[&bear].zone == Zone::Battlefield
        {
            // Reach-guard: the stack top must be the boon trigger WITH its
            // entrant stamped — otherwise a fire-without-stamp bug would
            // pass the no-token assertions below for the wrong reason.
            let top = runner
                .state()
                .stack
                .last()
                .expect("boon trigger on the stack");
            let StackEntryKind::TriggeredAbility { ability, .. } = &top.kind else {
                panic!("stack top must be the boon trigger, got {:?}", top.kind);
            };
            assert!(
                !ability.context.boon_trigger_batch_objects.is_empty(),
                "the boon trigger must stamp its entrant before Bolt is cast"
            );
            runner.cast(bolt).target_object(bear).resolve();
            bolted = true;
            continue;
        }
        match &runner.state().waiting_for {
            WaitingFor::Priority { .. } if runner.state().stack.is_empty() => break,
            WaitingFor::Priority { .. } => runner.pass_both_players(),
            other => panic!("unexpected prompt: {other:?}"),
        }
    }
    assert!(bolted, "the bear must have died in response");
    drain_stack(&mut runner);

    assert!(
        role_tokens(&runner).is_empty(),
        "no Role may be created without a legal host"
    );
    assert!(held_boons(&runner).is_empty());
}

#[test]
fn cross_card_notes_stay_independent() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dragonborn = scenario
        .add_creature_from_oracle(P0, "Dragonborn Immolator", 3, 3, DRAGONBORN)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let wall = scenario.add_creature_from_oracle(P1, "Wall", 0, 2, "").id();
    let mephits = scenario
        .add_spell_to_hand_from_oracle(P0, "Mephit's Enthusiasm", false, MEPHITS)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // Dragonborn dies noting its power 3; Mephit's notes excess 2. Notes
    // from different cards must not cross-contaminate their boons.
    runner.cast(bolt).target_object(dragonborn).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(3));
    runner.cast(mephits).target_object(wall).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, Some(2));
    assert_eq!(held_boons(&runner).len(), 2);
    assert_eq!(held_boon_captures(&runner), vec![Some(3), Some(2)]);

    // Both fire on the bear: 2 + 3 + 2.
    let bear_card = runner.state().objects[&bear].card_id;
    runner
        .act(GameAction::CastSpell {
            object_id: bear,
            card_id: bear_card,
            targets: vec![],
            payment_mode: engine::types::game_state::CastPaymentMode::Auto,
        })
        .expect("cast Bear");
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(7), Some(2)));
    let mut frozen = frozen_perpetual_pt(&runner, bear);
    frozen.sort_unstable();
    assert_eq!(frozen, vec![(2, 0), (3, 0)]);
    assert_no_dynamic_perpetual(&runner, bear);
    assert!(held_boons(&runner).is_empty());
}
