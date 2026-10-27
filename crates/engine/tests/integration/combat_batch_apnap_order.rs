//! Matt R15-3: simultaneous replacement prompts are made in APNAP order
//! (CR 101.4 + CR 616.1), not attack-declaration order.
//!
//! Fixture (3-player, P0 active): P0 attacks P1 with A and P2 with B (2/2s,
//! unblocked). P1 and P2 each control a Serra's Emissary naming Creature;
//! P0 controls Furnace of Rath. Each 2-damage event pauses on
//! [Furnace doubling, Emissary protection], chosen by the affected player.
//! Declaring in reverse seat order must still prompt P1 first: both runs
//! converge on the same prompt order and the same totals.
//!
//! Discriminating: pre-fix the batch keeps declaration order, so the
//! reversed run prompts P2 first.
//!
//! Furnace text is verbatim (MTGJSON Oracle, pinned in
//! `fixed_damage_amount_thresholds`); the Emissaries are real cards placed
//! directly, with the as-enters choice answered through production like
//! `selfless_squire_furnace_protection_ordering` (direct placement never runs
//! as-enters replacements, so the prompt is keyed to the permanent).

use engine::database::card_db::CardDatabase;
use engine::game::combat::AttackTarget;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::game::scenario_db::GameScenarioDbExt;
use engine::types::ability::{ChoiceType, TargetRef};
use engine::types::actions::GameAction;
use engine::types::card_type::CoreType;
use engine::types::events::GameEvent;
use engine::types::game_state::WaitingFor;
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::proposed_event::ProposedEvent;
use engine::types::zones::Zone;

use crate::support::exact_named_choice_source;
use crate::support::shared_card_db as load_db;

const P2: PlayerId = PlayerId(2);

/// Verbatim Furnace of Rath (shared with
/// `selfless_squire_furnace_protection_ordering`).
const FURNACE: &str = "If a source would deal damage to a permanent or player, it deals double that damage to that permanent or player instead.";

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    /// The affected player of each two-candidate prompt, in order.
    prompt_order: Vec<PlayerId>,
    lives: Vec<i32>,
}

struct Fixture {
    runner: GameRunner,
    a: ObjectId,
    b: ObjectId,
    furnace: ObjectId,
}

fn answer_emissary(runner: &mut GameRunner, emissary: ObjectId, player: PlayerId) {
    // The ETB Choose effect (a replacement) yields `WaitingFor::NamedChoice`
    // keyed to the entering permanent; answer Creature through production.
    let named_source = exact_named_choice_source(runner.state(), emissary);
    runner.state_mut().waiting_for = WaitingFor::NamedChoice {
        free_entry: None,
        player,
        choice_type: ChoiceType::card_type(),
        options: vec![
            "Artifact".to_string(),
            "Creature".to_string(),
            "Enchantment".to_string(),
            "Instant".to_string(),
            "Land".to_string(),
            "Planeswalker".to_string(),
            "Sorcery".to_string(),
        ],
        source: Some(named_source),
        persist_player: None,
    };
    runner
        .act(GameAction::ChooseOption {
            choice: "Creature".to_string(),
        })
        .expect("choosing Creature for the Emissary must resolve");
    assert_eq!(
        runner.state().objects[&emissary].chosen_card_type(),
        Some(CoreType::Creature),
        "reach guard: the Emissary must name Creature"
    );
}

fn setup(db: &CardDatabase) -> Fixture {
    let mut scenario = GameScenario::new_n_player(3, 0);
    scenario.at_phase(Phase::PreCombatMain);
    let a = scenario.add_creature(P0, "Ember A", 2, 2).id();
    let b = scenario.add_creature(P0, "Ember B", 2, 2).id();
    let emissary1 = scenario.add_real_card(P1, "Serra's Emissary", Zone::Battlefield, db);
    let emissary2 = scenario.add_real_card(P2, "Serra's Emissary", Zone::Battlefield, db);
    let furnace = scenario
        .add_enchantment_from_oracle(P0, "Furnace of Rath", FURNACE)
        .id();
    let mut runner = scenario.build();
    answer_emissary(&mut runner, emissary1, P1);
    answer_emissary(&mut runner, emissary2, P2);
    Fixture {
        runner,
        a,
        b,
        furnace,
    }
}

/// Drive combat with the given declaration order; answer every two-candidate
/// prompt protection-first and drain CR 616.1f remainders. Returns the prompt
/// order observed plus the final lives.
fn drive(fixture: &mut Fixture, reversed: bool) -> Outcome {
    let runner = &mut fixture.runner;
    let attacks = if reversed {
        vec![
            (fixture.b, AttackTarget::Player(P2)),
            (fixture.a, AttackTarget::Player(P1)),
        ]
    } else {
        vec![
            (fixture.a, AttackTarget::Player(P1)),
            (fixture.b, AttackTarget::Player(P2)),
        ]
    };
    let mut prompt_order = Vec::new();
    let mut log: Vec<GameEvent> = Vec::new();
    let mut declared = false;
    for _ in 0..128 {
        if matches!(
            runner.state().phase,
            Phase::EndCombat | Phase::PostCombatMain
        ) {
            break;
        }
        match runner.state().waiting_for.clone() {
            WaitingFor::DeclareAttackers { player, .. } => {
                assert_eq!(player, P0, "P0 is the active player");
                assert!(!declared, "attack declaration must occur only once");
                declared = true;
                log.extend(
                    runner
                        .declare_attackers(&attacks)
                        .expect("declare the attackers")
                        .events,
                );
            }
            WaitingFor::DeclareBlockers { .. } => {
                // No blockers anywhere; each defender declines in turn.
                log.extend(
                    runner
                        .declare_blockers(&[])
                        .expect("decline to block")
                        .events,
                );
            }
            WaitingFor::ReplacementChoice {
                player,
                candidate_count,
                candidates,
                ..
            } => {
                if candidate_count == 2 {
                    // The affected player chooses between the Furnace doubling
                    // and their Emissary's protection; the parked event must
                    // name that player's own damage.
                    let proposed_target = match runner
                        .state()
                        .pending_replacement
                        .as_ref()
                        .expect("a parked event must back the prompt")
                        .proposed
                        .clone()
                    {
                        ProposedEvent::Damage { target, .. } => target,
                        other => panic!("the prompt must park damage, got {other:?}"),
                    };
                    assert_eq!(
                        proposed_target,
                        TargetRef::Player(player),
                        "the prompt must name its chooser's own event"
                    );
                    assert!(
                        candidates.iter().any(|c| c.source_id == fixture.furnace),
                        "the Furnace doubling must be offered, got {candidates:?}"
                    );
                    let protection = candidates
                        .iter()
                        .position(|c| {
                            c.description
                                .starts_with("Prevent damage with protection from ")
                        })
                        .unwrap_or_else(|| {
                            panic!("the Emissary protection must be offered, got {candidates:?}")
                        });
                    prompt_order.push(player);
                    log.extend(
                        runner
                            .act(GameAction::ChooseReplacement { index: protection })
                            .expect("answer protection-first")
                            .events,
                    );
                } else {
                    assert_eq!(
                        candidate_count, 1,
                        "only CR 616.1f lone remainders may follow, got {candidates:?}"
                    );
                    log.extend(
                        runner
                            .act(GameAction::ChooseReplacement { index: 0 })
                            .expect("answer the lone remainder")
                            .events,
                    );
                }
            }
            WaitingFor::OrderTriggers { triggers, .. } => {
                let order: Vec<usize> = (0..triggers.len()).collect();
                log.extend(
                    runner
                        .act(GameAction::OrderTriggers { order })
                        .expect("order triggers")
                        .events,
                );
            }
            WaitingFor::Priority { .. } => {
                log.extend(
                    runner
                        .act(GameAction::PassPriority)
                        .expect("advance combat")
                        .events,
                );
            }
            other => panic!("unexpected combat prompt: {other:?}"),
        }
    }
    assert!(
        declared,
        "stall guard: attacks must actually have been declared"
    );
    assert!(
        matches!(
            runner.state().phase,
            Phase::EndCombat | Phase::PostCombatMain
        ),
        "combat must complete"
    );
    assert!(
        log.iter().all(|event| !matches!(
            event,
            GameEvent::DamageDealt {
                target: TargetRef::Player(_),
                ..
            }
        )),
        "no player may be dealt damage through"
    );
    let lives = runner
        .state()
        .players
        .iter()
        .map(|p| p.life)
        .collect::<Vec<_>>();
    Outcome {
        prompt_order,
        lives,
    }
}

#[test]
fn simultaneous_replacement_prompts_follow_apnap_not_declaration_order() {
    let db = load_db().expect("the card database must load");
    let mut forward = setup(db);
    let forward_outcome = drive(&mut forward, false);
    assert_eq!(
        forward_outcome.prompt_order,
        vec![P1, P2],
        "seat order already prompts P1 first"
    );
    assert_eq!(
        forward_outcome.lives,
        vec![20, 20, 20],
        "protection-first prevents every 2 (doubling a prevented 0 deals 0)"
    );

    let mut reversed = setup(db);
    let reversed_outcome = drive(&mut reversed, true);
    assert_eq!(
        reversed_outcome.prompt_order,
        vec![P1, P2],
        "reversed declaration must still prompt P1 first (APNAP)"
    );
    assert_eq!(
        reversed_outcome, forward_outcome,
        "both declaration orders converge on the same prompts and totals"
    );
}
