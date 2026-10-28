//! Bow to My Command: the end-step payment enforces total power 8 (CR 118.12).
//!
//! Oracle: "(An ongoing scheme remains face up until it's abandoned.)
//! As you set this scheme in motion, choose an opponent.
//! Creatures the chosen player controls can't attack you or planeswalkers you control.
//! At the beginning of your opponents' end step, they may tap any number of untapped creatures they control with total power 8 or greater. If they do, abandon this scheme."
//!
//! CR 118.12 identifies the tap as "a cost, paid when the spell or ability
//! resolves", so the head lowers to an aggregate `PayCost(TapCreatures)` paid
//! by the triggering opponent — never to a bare `SetTapState`, which would
//! drop the total-power restriction and abandon unconditionally.
//!
//! Two rows, both in a 2-player Archenemy game with Bow face up (P1 chosen):
//! * below-eight: P1's creatures total 7 — no payment prompt is offered, the
//!   trigger drains silently, and Bow stays face up.
//! * eight: P1's creatures total exactly 8 — P1 is prompted, pays by tapping
//!   both, and Bow is abandoned (face down to the scheme-deck bottom).
//!
//! Both rows seat a 10-power P0 creature as a reach guard: with the wrong
//! payer (the archenemy) the aggregate prompt would offer it, and the
//! below-eight row would become payable.
//!
//! Revert-to-red: lower the head back to `SetTapState` — the below-eight row
//! abandons Bow unconditionally. Rebind the payer to the controller — the
//! eight row offers P0's creature instead of P1's.

use engine::database::synthesis::synthesize_archenemy;
use engine::game::deck_loading::create_scheme_deck_card;
use engine::game::scenario::{GameRunner, GameScenario, P0, P1};
use engine::parser::parse_oracle_text;
use engine::types::ability::TapCreaturesSelectionMode;
use engine::types::actions::GameAction;
use engine::types::card::CardFace;
use engine::types::card_type::{CoreType, Supertype};
use engine::types::format::FormatConfig;
use engine::types::game_state::{PayCostKind, WaitingFor};
use engine::types::identifiers::ObjectId;
use engine::types::phase::Phase;

const BOW_TO_MY_COMMAND: &str = "(An ongoing scheme remains face up until it's abandoned.)\nAs you set this scheme in motion, choose an opponent.\nCreatures the chosen player controls can't attack you or planeswalkers you control.\nAt the beginning of your opponents' end step, they may tap any number of untapped creatures they control with total power 8 or greater. If they do, abandon this scheme.";

fn scheme_face(name: &str, oracle: &str) -> CardFace {
    let parsed = parse_oracle_text(oracle, name, &[], &["Scheme".to_string()], &[]);
    assert!(
        !format!("{parsed:?}").contains("Unimplemented"),
        "{name}: document is gapped"
    );
    let mut face = CardFace {
        name: name.to_string(),
        triggers: parsed.triggers,
        static_abilities: parsed.statics,
        ..Default::default()
    };
    face.card_type.core_types.push(CoreType::Scheme);
    face.card_type.supertypes.push(Supertype::Ongoing);
    synthesize_archenemy(&mut face);
    face
}

/// Shared setup: Bow set in motion with P1 chosen, P1's creatures on the
/// battlefield at the given powers, a P0 10/10 reach guard, P1 active in
/// precombat main. Returns the runner, Bow's id, P1's creatures, and P0's
/// beater.
fn setup_end_step_game(p1_powers: &[i32]) -> (GameRunner, ObjectId, Vec<ObjectId>, ObjectId) {
    let mut format = FormatConfig::archenemy();
    format.archenemy_player = Some(P0);
    let mut scenario = GameScenario::new_with_format(format, 2, 42);
    scenario.at_phase(Phase::Draw);
    let mut p1_creatures = Vec::new();
    for (i, power) in p1_powers.iter().enumerate() {
        p1_creatures.push(
            scenario
                .add_creature(P1, &format!("Hero Bear {i}"), *power, *power)
                .id(),
        );
    }
    let beater = scenario.add_creature(P0, "Archenemy Beater", 10, 10).id();
    for i in 0..7 {
        scenario.add_card_to_library_top(P0, &format!("P0 Filler {i}"));
        scenario.add_card_to_library_top(P1, &format!("P1 Filler {i}"));
    }
    let mut runner = scenario.build();

    let face = scheme_face("Bow to My Command", BOW_TO_MY_COMMAND);
    create_scheme_deck_card(runner.state_mut(), &face, P0);
    runner.state_mut().archenemy = Some(P0);
    runner.state_mut().active_player = P0;

    // CR 904.9: entering the archenemy's precombat main sets Bow in motion.
    runner.advance_to_phase(Phase::PreCombatMain);
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::NamedChoice { .. }),
        "expected the as-choice prompt, got {:?}",
        runner.state().waiting_for
    );
    runner
        .act(GameAction::ChooseOption {
            choice: "1".to_string(),
        })
        .expect("choosing P1 must be accepted");
    runner.advance_to_priority_window();
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "as-choice must drain to priority, got {:?}",
        runner.state().waiting_for
    );

    let bow = *runner
        .state()
        .command_zone
        .iter()
        .find(|id| runner.state().objects[*id].name == "Bow to My Command")
        .expect("Bow must be face up in the command zone");
    assert!(
        !runner.state().objects[&bow].face_down,
        "Bow must be face up after being set in motion"
    );

    // Hand P1 the turn at precombat main; the tests drive to P1's end step.
    runner.state_mut().active_player = P1;
    runner.state_mut().priority_player = P1;
    runner.state_mut().waiting_for = WaitingFor::Priority { player: P1 };
    (runner, bow, p1_creatures, beater)
}

/// Drive P1's precombat main through an empty combat into P1's end step.
fn advance_to_p1_end_step(runner: &mut GameRunner) {
    runner.advance_to_combat();
    runner
        .act(GameAction::DeclareAttackers {
            attacks: vec![],
            bands: vec![],
        })
        .expect("declare no attackers to cross combat");
    runner.advance_to_end_step();
    assert_eq!(
        runner.state().phase,
        Phase::End,
        "must reach P1's end step, got {:?}",
        runner.state().phase
    );
    assert_eq!(
        runner.state().active_player,
        P1,
        "the end step must be P1's"
    );
}

/// CR 118.12: P1's creatures total 7 — below the floor. The trigger fires and
/// drains with NO payment prompt (unpayable, so the optional is suppressed),
/// nothing taps, and Bow stays face up.
#[test]
fn bow_end_step_below_eight_offers_no_payment_and_keeps_bow_face_up() {
    let (mut runner, bow, p1_creatures, beater) = setup_end_step_game(&[4, 3]);
    advance_to_p1_end_step(&mut runner);

    let mut saw_trigger = false;
    let mut reached_optional = false;
    let mut reached_prompt = false;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OrderTriggers { triggers, .. } => {
                saw_trigger = true;
                runner
                    .act(GameAction::OrderTriggers {
                        order: (0..triggers.len()).collect(),
                    })
                    .unwrap();
            }
            WaitingFor::OptionalEffectChoice { .. } => {
                // Bug path: decline to drain, but the row still fails below.
                reached_optional = true;
                runner
                    .act(GameAction::DecideOptionalEffect { accept: false })
                    .unwrap();
            }
            WaitingFor::PayCost { .. } => {
                // Bug path: record and stop draining; the row fails below.
                reached_prompt = true;
                break;
            }
            WaitingFor::Priority { .. } => {
                if !runner.state().stack.is_empty() {
                    saw_trigger = true;
                }
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).unwrap();
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }

    assert!(
        saw_trigger,
        "Bow's end-step trigger must fire (else the row is vacuous)"
    );
    assert!(
        !reached_optional,
        "below eight total power, no optional payment may be offered"
    );
    assert!(
        !reached_prompt,
        "below eight total power, no tap-creatures prompt may appear"
    );
    assert!(
        runner.state().command_zone.contains(&bow),
        "Bow must stay face up in the command zone"
    );
    assert!(
        !runner.state().objects[&bow].face_down,
        "Bow must stay face up"
    );
    for id in &p1_creatures {
        assert!(
            !runner.state().objects[id].tapped,
            "nothing may tap when the floor is unreachable"
        );
    }
    assert!(
        !runner.state().objects[&beater].tapped,
        "the archenemy's creature must never be touched"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "clean drain back to priority, got {:?}",
        runner.state().waiting_for
    );
}

/// CR 118.12 + CR 601.2f: P1's creatures total exactly 8 — the floor. P1 is
/// prompted, taps both through the aggregate payment, and Bow is abandoned.
#[test]
fn bow_end_step_at_eight_taps_and_abandons() {
    let (mut runner, bow, p1_creatures, beater) = setup_end_step_game(&[4, 4]);
    advance_to_p1_end_step(&mut runner);

    let mut reached_optional = false;
    let mut reached_prompt = false;
    for _ in 0..60 {
        match runner.state().waiting_for.clone() {
            WaitingFor::OrderTriggers { triggers, .. } => {
                runner
                    .act(GameAction::OrderTriggers {
                        order: (0..triggers.len()).collect(),
                    })
                    .unwrap();
            }
            WaitingFor::OptionalEffectChoice { player, .. } => {
                reached_optional = true;
                assert_eq!(
                    player, P1,
                    "the triggering opponent announces the may, got {player:?}"
                );
                runner
                    .act(GameAction::DecideOptionalEffect { accept: true })
                    .unwrap();
            }
            WaitingFor::PayCost {
                player,
                kind,
                choices,
                resume,
                ..
            } => {
                reached_prompt = true;
                assert_eq!(player, P1, "the triggering opponent pays, got {player:?}");
                assert!(
                    matches!(kind, PayCostKind::TapCreatures { .. }),
                    "expected the aggregate tap-creatures payment, got {kind:?}"
                );
                assert!(
                    matches!(
                        kind,
                        PayCostKind::TapCreatures {
                            mode: TapCreaturesSelectionMode::Aggregate(_)
                        }
                    ),
                    "expected Aggregate selection semantics, got {kind:?}"
                );
                assert!(
                    matches!(resume, engine::types::game_state::CostResume::Resolution),
                    "expected a resolution-time payment, got {resume:?}"
                );
                // Reach guard: exactly P1's untapped creatures are offered —
                // never the archenemy's 10-power beater.
                assert_eq!(
                    choices.len(),
                    p1_creatures.len(),
                    "only P1's creatures are eligible, got {choices:?}"
                );
                for id in &p1_creatures {
                    assert!(
                        choices.contains(id),
                        "P1's creature {id:?} must be offered, got {choices:?}"
                    );
                }
                assert!(
                    !choices.contains(&beater),
                    "the archenemy's creature must not be offered, got {choices:?}"
                );
                runner
                    .act(GameAction::SelectCards {
                        cards: p1_creatures.clone(),
                    })
                    .unwrap();
            }
            WaitingFor::Priority { .. } => {
                if runner.state().stack.is_empty() {
                    break;
                }
                runner.act(GameAction::PassPriority).unwrap();
            }
            other => panic!("unexpected prompt {other:?}"),
        }
    }

    assert!(reached_optional, "the optional payment must be offered");
    assert!(reached_prompt, "the tap-creatures prompt must appear");
    for id in &p1_creatures {
        assert!(
            runner.state().objects[id].tapped,
            "P1's paid creatures must be tapped"
        );
    }
    assert!(
        !runner.state().objects[&beater].tapped,
        "the archenemy's creature must stay untapped"
    );
    // CR 701.33b / CR 314.2: abandoned to the bottom of the scheme deck, face
    // down, out of the command zone.
    assert!(
        !runner.state().command_zone.contains(&bow),
        "Bow must leave the command zone once abandoned"
    );
    assert!(
        runner.state().objects[&bow].face_down,
        "abandoned Bow must be face down"
    );
    assert!(
        runner.state().scheme_deck.contains(&bow),
        "abandoned Bow must be back in the scheme deck"
    );
    assert!(
        matches!(runner.state().waiting_for, WaitingFor::Priority { .. }),
        "clean drain back to priority, got {:?}",
        runner.state().waiting_for
    );
}
