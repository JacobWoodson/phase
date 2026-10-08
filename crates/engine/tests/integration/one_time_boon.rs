//! Digital-only Alchemy (no CR entry): one-time boons (issue #7495).
//!
//! Runtime behavior: a grant installs a Persistent one-shot `WhenNextEvent`
//! delayed trigger for its holder; the next matching event fires it once and
//! consumes it; "if you have a boon" reads the held list; an intervening-`if`
//! on the granted trigger is checked at fire time and again at resolution
//! (CR 603.4), with a false gate consuming the single occurrence (CR 603.7b).

use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::types::ability::{
    DelayedTriggerCondition, DelayedTriggerLifetime, PerpetualModification, TargetRef,
};
use engine::types::actions::{GameAction, ResolutionOptionalPaymentChoice};
use engine::types::counter::CounterType;
use engine::types::game_state::WaitingFor;
use engine::types::keywords::KeywordKind;
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
        .filter(|dt| dt.is_boon)
        .map(|dt| dt.controller)
        .collect()
}

/// The frozen perpetual P/T records on an object: dynamic deltas evaluate
/// once at application time, so only plain `ModifyPowerToughness` records
/// may persist — never a live `ModifyPowerToughnessDynamic`.
fn frozen_perpetual_pt(runner: &GameRunner, id: ObjectId) -> Vec<(i32, i32)> {
    runner.state().objects[&id]
        .perpetual_mods
        .iter()
        .filter_map(|modification| match modification {
            PerpetualModification::ModifyPowerToughness {
                power_delta,
                toughness_delta,
            } => Some((*power_delta, *toughness_delta)),
            _ => None,
        })
        .collect()
}

fn assert_no_dynamic_perpetual(runner: &GameRunner, id: ObjectId) {
    assert!(
        !runner.state().objects[&id]
            .perpetual_mods
            .iter()
            .any(|m| matches!(m, PerpetualModification::ModifyPowerToughnessDynamic { .. })),
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
        .filter(|dt| dt.is_boon)
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
fn dragonborn_zero_power_grants_boon_without_noting() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    let dragonborn = scenario
        .add_creature_from_oracle(P0, "Dragonborn Immolator", 0, 3, DRAGONBORN)
        .id();
    let bolt = scenario
        .add_spell_to_hand_from_oracle(P0, "Bolt", true, BOLT)
        .id();
    let bear = scenario
        .add_creature_to_hand_from_oracle(P0, "Grizzly Bears", 2, 2, "")
        .id();
    let mut runner = scenario.build();

    // Power 0 fails the note gate, but the grant is a separate sentence.
    runner.cast(bolt).target_object(dragonborn).resolve();
    drain_stack(&mut runner);
    assert_eq!(runner.state().players[0].noted_number, None);
    assert_eq!(
        held_boons(&runner),
        vec![P0],
        "the grant is ungated by the note condition"
    );

    // An un-noted X reads as 0: the pump lands as a frozen +0/+0.
    runner.cast(bear).resolve();
    drain_stack(&mut runner);
    let entered = &runner.state().objects[&bear];
    assert_eq!((entered.power, entered.toughness), (Some(2), Some(2)));
    assert_eq!(frozen_perpetual_pt(&runner, bear), vec![(0, 0)]);
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
fn second_excess_note_overwrites_first() {
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
        "a new note overwrites"
    );
    assert_eq!(held_boons(&runner).len(), 2);

    // Both boons fire on the one cast, each reading the latest note. Two
    // simultaneous triggers need manual driving: the cast driver's commit
    // loop does not order triggers.
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
    assert!(held_boons(&runner).is_empty());
}
