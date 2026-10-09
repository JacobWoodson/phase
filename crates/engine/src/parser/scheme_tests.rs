//! Verbatim full-text parse tests for Archenemy scheme cards (3v1 Commander
//! support). Each test parses the card's complete Oracle text via
//! `parse_oracle_text` and asserts the whole document is gap-free: no
//! `Effect::Unimplemented` in any ability or trigger body, and no
//! `TriggerMode::Unknown`.
//!
//! Oracle texts are verbatim from Scryfall/MTGJSON — never paraphrased, since
//! a paraphrase can take a different parser branch than the real card.

use super::has_unimplemented;
use super::parse_oracle_text;
use crate::types::ability::Effect;
use crate::types::triggers::TriggerMode;

fn scheme_types() -> (Vec<String>, Vec<String>) {
    (vec!["Scheme".to_string()], vec![])
}

/// Parse a scheme's full Oracle text and assert the document is gap-free.
fn assert_scheme_clean(name: &str, oracle_text: &str) {
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle_text, name, &[], &types, &subtypes);
    for ability in &parsed.abilities {
        assert!(
            !has_unimplemented(ability),
            "{name}: ability is gapped: {:?}",
            ability.effect
        );
    }
    for trigger in &parsed.triggers {
        assert!(
            !matches!(trigger.mode, TriggerMode::Unknown(_)),
            "{name}: trigger mode is Unknown: {:?}",
            trigger.mode
        );
        if let Some(execute) = trigger.execute.as_deref() {
            assert!(
                !has_unimplemented(execute),
                "{name}: trigger body is gapped: {:?}",
                execute.effect
            );
        }
    }
    for static_def in &parsed.statics {
        let debug = format!("{static_def:?}");
        assert!(
            !debug.contains("Unimplemented"),
            "{name}: static is gapped: {debug}"
        );
    }
    for replacement in &parsed.replacements {
        let debug = format!("{replacement:?}");
        assert!(
            !debug.contains("Unimplemented"),
            "{name}: replacement is gapped: {debug}"
        );
    }
}

/// The abandon body must lower to exactly `Effect::AbandonScheme` (not merely
/// non-Unimplemented).
fn assert_abandon_body(name: &str, oracle_text: &str) {
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle_text, name, &[], &types, &subtypes);
    let mut found = false;
    for trigger in &parsed.triggers {
        let mut cursor = trigger.execute.as_deref();
        while let Some(link) = cursor {
            if matches!(*link.effect, Effect::AbandonScheme) {
                found = true;
            }
            cursor = link.sub_ability.as_deref();
        }
    }
    assert!(
        found,
        "{name}: no Effect::AbandonScheme in any trigger body"
    );
}

#[test]
fn the_very_soil_shall_shake_is_supported() {
    let oracle = "(An ongoing scheme remains face up until it's abandoned.)\nCreatures you control get +2/+2 and have trample.\nWhen a creature you control dies, abandon this scheme.";
    assert_scheme_clean("The Very Soil Shall Shake", oracle);
    assert_abandon_body("The Very Soil Shall Shake", oracle);
}

#[test]
fn the_fate_of_the_flammable_is_supported() {
    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, this scheme deals 6 damage to that player. If the player chooses others, this scheme deals 3 damage to each of your other opponents.";
    assert_scheme_clean("The Fate of the Flammable", oracle);
}

/// CR 608.2d + CR 109.4 + CR 115.1: "target opponent chooses self or others"
/// binds the announced target as the `Choose` chooser — never the controller,
/// and never dropped. (Pre-fix, the targeted chooser was silently discarded
/// and the archenemy chose for the opponent.)
#[test]
fn fate_of_the_flammable_binds_targeted_chooser() {
    use crate::types::ability::{ChoiceType, ControllerRef};

    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, this scheme deals 6 damage to that player. If the player chooses others, this scheme deals 3 damage to each of your other opponents.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "The Fate of the Flammable", &[], &types, &subtypes);
    assert_eq!(parsed.triggers.len(), 1);
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let Effect::Choose {
        choice_type,
        chooser,
        persist,
        ..
    } = &*root.effect
    else {
        panic!("expected Choose head, got {:?}", root.effect);
    };
    assert!(
        matches!(choice_type, ChoiceType::Labeled { options } if options == &["Self".to_string(), "Others".to_string()]),
        "expected Labeled[Self, Others], got {choice_type:?}"
    );
    assert_eq!(
        chooser,
        &ControllerRef::TargetOpponent,
        "chooser must be the announced target opponent, got {chooser:?}"
    );
    assert!(persist, "label must persist for the branch gates");
}

/// CR 607.2d + CR 608.2c + CR 115.1: both Fate branches carry their label
/// gate, and the self branch's recipient anaphor names the announced target.
#[test]
fn fate_of_the_flammable_branches_hit_self_source_and_anaphor_targets() {
    use crate::types::ability::{AbilityCondition, TargetFilter};

    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, this scheme deals 6 damage to that player. If the player chooses others, this scheme deals 3 damage to each of your other opponents.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "The Fate of the Flammable", &[], &types, &subtypes);
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let self_branch = root.sub_ability.as_deref().expect("self branch");
    let Effect::DealDamage { target, .. } = &*self_branch.effect else {
        panic!(
            "expected DealDamage self branch, got {:?}",
            self_branch.effect
        );
    };
    assert_eq!(
        target,
        &TargetFilter::ParentTarget,
        "self-branch recipient must be the announced target, got {target:?}"
    );
    assert_eq!(
        self_branch.condition,
        Some(AbilityCondition::ChosenLabelIs {
            label: "Self".to_string()
        }),
        "self branch must be label-gated, got {:?}",
        self_branch.condition
    );
    let others_branch = self_branch.sub_ability.as_deref().expect("others branch");
    assert_eq!(
        others_branch.condition,
        Some(AbilityCondition::ChosenLabelIs {
            label: "Others".to_string()
        }),
        "others branch must be label-gated, got {:?}",
        others_branch.condition
    );
}

/// CR 102.2 + CR 608.2c + CR 109.4 + CR 120.3: the Others branch of The Fate of
/// the Flammable ("this scheme deals 3 damage to each of your other opponents")
/// lowers to `DamageEachPlayer` over opponents-except-the-targeted-chooser —
/// never to `DamageAll` over an empty object filter (which would hit every
/// permanent) and never to a bare `Opponent` scope (which would hit the
/// chooser, who chose Others precisely to dodge the damage).
#[test]
fn fate_of_the_flammable_others_branch_excludes_targeted_chooser() {
    use crate::types::ability::{PlayerFilter, QuantityExpr};

    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, this scheme deals 6 damage to that player. If the player chooses others, this scheme deals 3 damage to each of your other opponents.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "The Fate of the Flammable", &[], &types, &subtypes);
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let self_branch = root.sub_ability.as_deref().expect("self branch");
    let others_branch = self_branch.sub_ability.as_deref().expect("others branch");
    let Effect::DamageEachPlayer {
        amount,
        player_filter,
    } = &*others_branch.effect
    else {
        panic!(
            "expected DamageEachPlayer others branch, got {:?}",
            others_branch.effect
        );
    };
    assert!(
        matches!(amount, QuantityExpr::Fixed { value: 3 }),
        "others branch must deal 3, got {amount:?}"
    );
    assert_eq!(
        player_filter,
        &PlayerFilter::OpponentExcept {
            exclude: Box::new(PlayerFilter::ParentPlayerTarget),
        },
        "others branch must exclude the targeted chooser, got {player_filter:?}"
    );
}

/// Shared head assertion for the self-or-others class: the `Choose` binds the
/// announced target opponent as chooser and persists the label.
fn assert_self_or_others_head(name: &str, oracle_text: &str) {
    use crate::types::ability::{ChoiceType, ControllerRef};

    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle_text, name, &[], &types, &subtypes);
    assert_eq!(parsed.triggers.len(), 1, "{name}: one trigger");
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let Effect::Choose {
        choice_type,
        chooser,
        persist,
        ..
    } = &*root.effect
    else {
        panic!("{name}: expected Choose head, got {:?}", root.effect);
    };
    assert!(
        matches!(choice_type, ChoiceType::Labeled { options } if options == &["Self".to_string(), "Others".to_string()]),
        "{name}: expected Labeled[Self, Others], got {choice_type:?}"
    );
    assert_eq!(
        chooser,
        &ControllerRef::TargetOpponent,
        "{name}: chooser must be the announced target opponent, got {chooser:?}"
    );
    assert!(persist, "{name}: label must persist for the branch gates");
}

#[test]
fn surrender_your_thoughts_is_supported() {
    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, that player discards four cards. If the player chooses others, each of your other opponents discards two cards.";
    assert_scheme_clean("Surrender Your Thoughts", oracle);
    assert_self_or_others_head("Surrender Your Thoughts", oracle);
}

/// CR 102.2 + CR 608.2c + CR 109.4: the Others branch of Surrender Your
/// Thoughts ("each of your other opponents discards two cards") carries a
/// `player_scope` of opponents-except-the-targeted-chooser — never a bare
/// `Opponent` scope (which would make the chooser discard after choosing
/// Others precisely to dodge it).
#[test]
fn surrender_others_branch_scope_excludes_targeted_chooser() {
    use crate::types::ability::PlayerFilter;

    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, that player discards four cards. If the player chooses others, each of your other opponents discards two cards.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "Surrender Your Thoughts", &[], &types, &subtypes);
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let self_branch = root.sub_ability.as_deref().expect("self branch");
    let others_branch = self_branch.sub_ability.as_deref().expect("others branch");
    assert!(
        matches!(&*others_branch.effect, Effect::Discard { .. }),
        "expected Discard others branch, got {:?}",
        others_branch.effect
    );
    assert_eq!(
        others_branch.player_scope,
        Some(PlayerFilter::OpponentExcept {
            exclude: Box::new(PlayerFilter::ParentPlayerTarget),
        }),
        "others branch must scope to opponents-except-chooser, got {:?}",
        others_branch.player_scope
    );
}

/// CR 608.2c + CR 115.1: subject-position "that player" after the targeted
/// choice names the announced target (the subject-path half of Fix B; Fate
/// covers the recipient path).
#[test]
fn surrender_self_branch_subject_names_announced_target() {
    use crate::types::ability::TargetFilter;

    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, that player discards four cards. If the player chooses others, each of your other opponents discards two cards.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "Surrender Your Thoughts", &[], &types, &subtypes);
    let root = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    let self_branch = root.sub_ability.as_deref().expect("self branch");
    let Effect::Discard { target, .. } = &*self_branch.effect else {
        panic!("expected Discard self branch, got {:?}", self_branch.effect);
    };
    assert_eq!(
        target,
        &TargetFilter::ParentTarget,
        "self-branch subject must be the announced target, got {target:?}"
    );
}

#[test]
fn feed_the_machine_is_supported() {
    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, the player sacrifices two creatures of their choice. If the player chooses others, each of your other opponents sacrifices a creature of their choice.";
    assert_scheme_clean("Feed the Machine", oracle);
    assert_self_or_others_head("Feed the Machine", oracle);
}

#[test]
fn may_civilization_collapse_is_supported() {
    let oracle = "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, that player sacrifices two lands of their choice. If the player chooses others, each of your other opponents sacrifices a land of their choice.";
    assert_scheme_clean("May Civilization Collapse", oracle);
    assert_self_or_others_head("May Civilization Collapse", oracle);
}

/// CR 102.2 + CR 608.2c + CR 109.4: the sacrifice siblings' Others branches
/// ("each of your other opponents sacrifices …") carry the same
/// opponents-except-the-targeted-chooser `player_scope` as Surrender's — the
/// strip arm is verb-agnostic.
#[test]
fn sacrifice_siblings_others_branch_scope_excludes_targeted_chooser() {
    use crate::types::ability::PlayerFilter;

    for (name, oracle) in [
        (
            "Feed the Machine",
            "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, the player sacrifices two creatures of their choice. If the player chooses others, each of your other opponents sacrifices a creature of their choice.",
        ),
        (
            "May Civilization Collapse",
            "When you set this scheme in motion, target opponent chooses self or others. If that player chooses self, that player sacrifices two lands of their choice. If the player chooses others, each of your other opponents sacrifices a land of their choice.",
        ),
    ] {
        let (types, subtypes) = scheme_types();
        let parsed = parse_oracle_text(oracle, name, &[], &types, &subtypes);
        let root = parsed.triggers[0]
            .execute
            .as_deref()
            .expect("trigger has a body");
        let self_branch = root.sub_ability.as_deref().expect("self branch");
        let others_branch = self_branch
            .sub_ability
            .as_deref()
            .expect("others branch");
        assert_eq!(
            others_branch.player_scope,
            Some(PlayerFilter::OpponentExcept {
                exclude: Box::new(PlayerFilter::ParentPlayerTarget),
            }),
            "{name}: others branch must scope to opponents-except-chooser, got {:?}",
            others_branch.player_scope
        );
    }
}

/// CR 607.2d + CR 608.2c: the self-or-others branch gate — "If that player
/// chooses self, …" must carry `AbilityCondition::ChosenLabelIs`, never drop
/// the guard (the pre-fix behavior silently fired both branches always).
#[test]
fn if_that_player_chooses_self_carries_label_condition() {
    let def = crate::parser::oracle_effect::parse_effect_chain(
        "If that player chooses self, this scheme deals 6 damage to that player.",
        crate::types::ability::AbilityKind::Spell,
    );
    assert_eq!(
        def.condition,
        Some(crate::types::ability::AbilityCondition::ChosenLabelIs {
            label: "Self".to_string()
        }),
        "branch must be gated on the chosen label, got {:?}",
        def.condition
    );
}

/// Which of You Burns Brightest? stays HONESTLY gapped: the "and each
/// creature that player or that planeswalker's controller controls"
/// continuation is a disjunctive target-anchored fan-out no splitter consumes.
/// The pre-guard behavior parsed the primary DealDamage alone and silently
/// dropped half the damage (assert_scheme_clean passed on a half-wrong rule).
/// The `try_parse_damage` compound-connector guard declines the primary so the
/// clause fails closed to Unimplemented instead. A future target-plus-each
/// fan-out (Radiance-shaped: DealDamage primary + DamageAll sub over
/// `Or[Typed{TargetPlayer}, Typed{ParentTargetController}]`) reclaims it.
#[test]
fn which_of_you_burns_brightest_fails_closed_on_creature_continuation() {
    let oracle = "When you set this scheme in motion, you may pay {X}. If you do, this scheme deals X damage to target opponent or planeswalker and each creature that player or that planeswalker's controller controls.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(
        oracle,
        "Which of You Burns Brightest?",
        &[],
        &types,
        &subtypes,
    );
    let body = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    assert!(
        has_unimplemented(body),
        "Burns' dropped continuation must fail closed to Unimplemented, got {body:?}"
    );
}

#[test]
fn your_inescapable_doom_is_supported() {
    let oracle = "(An ongoing scheme remains face up.)\nAt the beginning of your end step, put a doom counter on this scheme, then this scheme deals damage equal to the number of doom counters on it to the opponent with the highest life total among your opponents. If two or more players are tied for highest life total, you choose one.";
    assert_scheme_clean("Your Inescapable Doom", oracle);
}

/// I Bask in Your Silent Awe stays HONESTLY gapped: "since your last turn
/// ended" is a cross-turn window the turn journals cannot express (only two
/// cards in the pool use one — this and Premature Burial, on different
/// surfaces). Approximating it as "this turn" would abandon every upkeep
/// unconditionally (silently wrong); the trigger must fail closed with
/// `unparsed_condition`, never widen.
#[test]
fn i_bask_in_your_silent_awe_fails_closed_on_since_window() {
    let oracle = "(An ongoing scheme remains face up until it's abandoned.)\nEach opponent can't cast more than one spell each turn.\nAt the beginning of your upkeep, if no opponent cast a spell since your last turn ended, abandon this scheme.";
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle, "I Bask in Your Silent Awe", &[], &types, &subtypes);
    assert_eq!(parsed.triggers.len(), 1);
    let execute = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("abandon trigger body");
    match &*execute.effect {
        Effect::Unimplemented { name, .. } => assert_eq!(
            name, "unparsed_condition",
            "since-window must fail closed as unparsed_condition, got gap {name:?}"
        ),
        other => panic!("since-window must not widen to a bare abandon, got {other:?}"),
    }
}

#[test]
fn i_know_all_i_see_all_is_supported() {
    let oracle = "(An ongoing scheme remains face up until it's abandoned.)\nUntap all permanents you control during each opponent's untap step.\nAt the beginning of each end step, if three or more cards were put into your graveyard this turn from anywhere, abandon this scheme.";
    assert_scheme_clean("I Know All, I See All", oracle);
    assert_abandon_body("I Know All, I See All", oracle);
}

#[test]
fn nothing_can_stop_me_now_is_supported() {
    let oracle = "(An ongoing scheme remains face up until it's abandoned.)\nIf a source an opponent controls would deal damage to you, prevent 1 of that damage.\nAt the beginning of each end step, if you've been dealt 5 or more damage this turn, abandon this scheme.";
    assert_scheme_clean("Nothing Can Stop Me Now", oracle);
    assert_abandon_body("Nothing Can Stop Me Now", oracle);
}

#[test]
fn because_i_have_willed_it_is_supported() {
    let oracle = "(An ongoing scheme remains face up until it's abandoned.)\nSpells you cast cost {1} less to cast.\nAt the beginning of your opponents' end step, if they cast four or more spells this turn, abandon this scheme.";
    assert_scheme_clean("Because I Have Willed It", oracle);
    assert_abandon_body("Because I Have Willed It", oracle);
}

/// CR 108.3 + CR 508.1d: the attack-owner rider class — "Each of them attacks
/// its owner" / "Each of those creatures attacks its owner" lowers to
/// `ForceAttack` over the inherited subject with the per-member
/// `AffectedObjectOwner` anchor, never to a bare `MustAttack` (which would
/// drop the defender) and never to `Unimplemented`. The bound population is
/// `ParentTarget` when the head clause names no published set ("Each of
/// them" over a gain-control head), or the chain's published tracked set when
/// it does ("Each of those creatures" over a reanimation head, whose move
/// publishes tracked set 0 — the sentinel the resolver reads at match time).
fn assert_attack_owner_tail(
    name: &str,
    oracle_text: &str,
    duration: crate::types::ability::Duration,
    expected_target: crate::types::ability::TargetFilter,
) {
    let (types, subtypes) = scheme_types();
    let parsed = parse_oracle_text(oracle_text, name, &[], &types, &subtypes);
    let mut cursor = parsed.triggers[0]
        .execute
        .as_deref()
        .expect("trigger has a body");
    while let Some(next) = cursor.sub_ability.as_deref() {
        cursor = next;
    }
    let Effect::ForceAttack {
        target,
        required_defender,
        scope,
        duration: effect_duration,
    } = &*cursor.effect
    else {
        panic!("{name}: tail must be ForceAttack, got {:?}", cursor.effect);
    };
    assert_eq!(
        target, &expected_target,
        "{name}: rider binds {expected_target:?}, got {target:?}"
    );
    assert_eq!(
        required_defender,
        &crate::types::ability::TargetFilter::AffectedObjectOwner,
        "{name}: defender must be the per-member owner anchor, got {required_defender:?}"
    );
    assert_eq!(
        scope,
        &crate::types::ability::EffectScope::All,
        "{name}: rider takes scope All (no targeting slots), got {scope:?}"
    );
    assert_eq!(
        effect_duration, &duration,
        "{name}: window maps to {duration:?}, got {effect_duration:?}"
    );
}

#[test]
fn my_crushing_masterstroke_is_supported() {
    let oracle = "When you set this scheme in motion, gain control of all nonland permanents your opponents control until end of turn. Untap those permanents. They gain haste until end of turn. Each of them attacks its owner this turn if able.";
    assert_scheme_clean("My Crushing Masterstroke", oracle);
    assert_attack_owner_tail(
        "My Crushing Masterstroke",
        oracle,
        crate::types::ability::Duration::UntilEndOfTurn,
        crate::types::ability::TargetFilter::ParentTarget,
    );
}

#[test]
fn the_dead_shall_serve_is_supported() {
    let oracle = "When you set this scheme in motion, for each opponent, put up to one target creature card from that player's graveyard onto the battlefield under your control. Each of those creatures attacks its owner each combat if able.";
    assert_scheme_clean("The Dead Shall Serve", oracle);
    assert_attack_owner_tail(
        "The Dead Shall Serve",
        oracle,
        crate::types::ability::Duration::Permanent,
        crate::types::ability::TargetFilter::TrackedSet {
            id: crate::types::identifiers::TrackedSetId(0),
        },
    );
}
