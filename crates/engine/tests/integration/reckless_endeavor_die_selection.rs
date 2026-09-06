//! Multi-die roll with per-die result selection — the Endeavor cycle class.
//!
//! Oracle (Reckless Endeavor):
//! > Roll two d12 and choose one result. Reckless Endeavor deals damage equal
//! > to that result to each creature. Then create a number of Treasure tokens
//! > equal to the other result.
//!
//! CR 706.4 governs the shape: a die-rolling ability with no results table
//! indicates in its own printed text how the results are used. Here the text
//! apportions TWO results between two later clauses via a choice the roll's
//! controller makes on resolution (CR 608.2d). Both results are consumed, so
//! this is an apportionment, NOT an ignored roll (CR 706.6).

use engine::game::scenario::{CastCommit, GameRunner, GameScenario, P0, P1};
use engine::parser::oracle::ParsedAbilities;
use engine::parser::parse_oracle_text;
use engine::types::ability::{
    CastPermissionConstraint, DieResultSelection, Effect, FilterProp, QuantityExpr, QuantityRef,
    TargetFilter, TypeFilter,
};
use engine::types::actions::GameAction;
use engine::types::card_type::{CoreType, Supertype};
use engine::types::events::GameEvent;
use engine::types::identifiers::ObjectId;
use engine::types::mana::ManaCost;
use engine::types::phase::Phase;
use engine::types::player::PlayerId;
use engine::types::zones::Zone;
use engine::types::WaitingFor;

const RECKLESS: &str = "Roll two d12 and choose one result. Reckless Endeavor deals damage \
equal to that result to each creature. Then create a number of Treasure tokens equal to the \
other result.";

const ARCANE: &str = "Roll two d8 and choose one result. Draw cards equal to that result. Then \
you may cast an instant or sorcery spell with mana value less than or equal to the other result \
from your hand without paying its mana cost.";

const WILD: &str = "Roll two d4 and choose one result. Create a number of 3/3 green Beast \
creature tokens equal to that result. Then search your library for a number of basic land cards \
equal to the other result, put them onto the battlefield tapped, then shuffle.";

/// Pass priority until the committed roll suspends for its die choice, and
/// return the rolled results.
///
/// Every apportioned card in the cycle reaches this same boundary, so the tests
/// below differ only in what happens AFTER the choice — which is the axis under
/// test.
fn drive_to_die_choice(cast: &mut CastCommit<'_>) -> Vec<u8> {
    for _ in 0..16 {
        if let WaitingFor::DieResultChoice { results, .. } = cast.state().waiting_for.clone() {
            return results;
        }
        cast.act(GameAction::PassPriority)
            .expect("passing priority to resolve the roll must be legal");
    }
    panic!(
        "never reached the die choice; parked at {:?}",
        cast.state().waiting_for
    )
}

/// Promote already-seeded library cards into basic land cards.
///
/// CR 205.2a + CR 205.4a: "basic land card" is matched by the `Land` card type
/// and the `Basic` supertype, not by the card's name. A generic library card
/// named "Forest" carries neither, so a basic-land search finds no candidates
/// and never raises its prompt at all — which would make this test vacuous
/// rather than failing.
fn make_library_cards_basic_lands(runner: &mut GameRunner, player: PlayerId, name: &str) {
    let ids: Vec<ObjectId> = runner
        .state()
        .players
        .iter()
        .find(|p| p.id == player)
        .expect("player must exist")
        .library
        .iter()
        .copied()
        .collect();
    let state = runner.state_mut();
    for id in ids {
        let obj = state.objects.get_mut(&id).expect("library card must exist");
        if obj.name != name {
            continue;
        }
        obj.card_types.core_types.push(CoreType::Land);
        obj.card_types.supertypes.push(Supertype::Basic);
        obj.card_types.subtypes.push(name.to_string());
        obj.base_card_types = obj.card_types.clone();
    }
}

fn parse(text: &str, name: &str) -> ParsedAbilities {
    parse_oracle_text(text, name, &[], &["Sorcery".to_string()], &[])
}

/// Flatten a chain into its effects, in written order (CR 608.2c).
fn chain(abilities: &ParsedAbilities) -> Vec<Effect> {
    let mut out = Vec::new();
    let mut node = abilities.abilities.first();
    while let Some(def) = node {
        out.push((*def.effect).clone());
        node = def.sub_ability.as_deref();
    }
    out
}

/// Collect every `QuantityRef` an expression reaches, so assertions survive
/// arithmetic wrappers ("twice that result").
fn refs_of(expr: &QuantityExpr) -> Vec<QuantityRef> {
    let mut found = Vec::new();
    expr.any_ref(&mut |qty| {
        found.push(qty.clone());
        false
    });
    found
}

fn binds_a_die_result(effect: &Effect) -> bool {
    !die_selections_of(effect).is_empty()
}

/// Every die this effect names, across ALL its quantity slots - its own amount,
/// its entry counters, its target-filter thresholds, and its cast-permission
/// constraint.
///
/// `for_each_quantity_expr` reaches the effect's own slots; the filter and
/// constraint thresholds are separate authorities that gate the same choice
/// point, so a test that reads only one of them cannot tell a fully-bound
/// clause from a half-bound one (an unbound threshold resolves to 0 and empties
/// the legal pool even when the effect's own amount is correct).
fn die_selections_of(effect: &Effect) -> Vec<DieResultSelection> {
    let mut found = Vec::new();
    let mut collect = |expr: &QuantityExpr| {
        for q in refs_of(expr) {
            if let QuantityRef::DieResultSelected { selection } = q {
                found.push(selection);
            }
        }
    };
    effect.for_each_quantity_expr(&mut collect);

    // `Effect::target_filter()` is deliberately scoped to PLAYER-SELECTABLE
    // targets (CR 115.1), so it returns `None` for the mass-population filters
    // that carry these thresholds ("each creature with power >= that result").
    // Read the population slot directly rather than through an accessor that
    // answers a different question.
    let population = match effect {
        Effect::DestroyAll { target, .. }
        | Effect::DamageAll { target, .. }
        | Effect::PutCounterAll { target, .. }
        | Effect::CastFromZone { target, .. } => Some(target),
        _ => effect.target_filter(),
    };
    if let Some(filter) = population {
        for expr in filter_quantities(filter) {
            collect(&expr);
        }
    }

    if let Effect::CastFromZone {
        constraint: Some(CastPermissionConstraint::ManaValue { value, .. }),
        ..
    } = effect
    {
        collect(value);
    }
    found
}

/// The comparison thresholds a filter carries, flattened through its boolean
/// combinators.
fn filter_quantities(filter: &TargetFilter) -> Vec<QuantityExpr> {
    let mut out = Vec::new();
    match filter {
        TargetFilter::Typed(typed) => {
            for prop in &typed.properties {
                match prop {
                    FilterProp::Cmc { value, .. } | FilterProp::PtComparison { value, .. } => {
                        out.push(value.clone());
                    }
                    FilterProp::Counters { count, .. } => out.push(count.clone()),
                    _ => {}
                }
            }
        }
        TargetFilter::And { filters } | TargetFilter::Or { filters } => {
            for member in filters {
                out.extend(filter_quantities(member));
            }
        }
        TargetFilter::Not { filter } => out.extend(filter_quantities(filter)),
        _ => {}
    }
    out
}

/// CR 706.4: the apportioned roll records its arity, and the two later clauses
/// read DIFFERENT dice.
#[test]
fn reckless_endeavor_apportions_two_die_results() {
    let parsed = parse(RECKLESS, "Reckless Endeavor");
    let effects = chain(&parsed);

    // Reach-guard: assert the positive shape, so the "no Unimplemented" check
    // below cannot pass vacuously on a chain that failed to parse.
    assert_eq!(
        effects.len(),
        3,
        "expected roll + damage + tokens: {effects:?}"
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Unimplemented { .. })),
        "no clause may remain unimplemented: {effects:?}"
    );

    // CR 706.1 + CR 706.4: two twelve-sided dice, apportioned.
    match &effects[0] {
        Effect::RollDie {
            count,
            sides,
            results,
            selection,
            ..
        } => {
            assert_eq!(*count, QuantityExpr::Fixed { value: 2 });
            assert_eq!(*sides, 12);
            // CR 706.3b: an apportioned roll never carries a results table.
            assert!(results.is_empty());
            assert_eq!(*selection, Some(2));
        }
        other => panic!("expected RollDie, got {other:?}"),
    }

    // CR 706.4: "that result" is the CHOSEN die.
    let Effect::DamageAll { amount, .. } = &effects[1] else {
        panic!("expected DamageAll, got {:?}", effects[1]);
    };
    assert_eq!(
        refs_of(amount),
        vec![QuantityRef::DieResultSelected {
            selection: DieResultSelection::Chosen
        }],
    );

    // CR 706.4: "the other result" is the REMAINING die — a different one.
    let Effect::Token { count, .. } = &effects[2] else {
        panic!("expected Token, got {:?}", effects[2]);
    };
    assert_eq!(
        refs_of(count),
        vec![QuantityRef::DieResultSelected {
            selection: DieResultSelection::Other
        }],
    );
}

/// CR 706.4: the binding is driven by the printed grammar, so the whole
/// Endeavor cycle apportions - this is a class, not a card.
///
/// Each card states the die EVERY post-roll clause must name, in chain order,
/// rather than asserting that *some* clause bound *something*. An `any()` test
/// is satisfied by a single correctly-bound clause and so cannot see a clause
/// whose quantity slot the rebinding pass failed to reach - which is precisely
/// the defect class this cycle is meant to prove absent. Per-clause
/// expectations turn each such miss red.
///
/// `Chosen` is "that result"; `Other` is "the other result". A clause listed
/// with SEVERAL entries names the same die from several slots (Arcane
/// Endeavor's free cast gates its pool through both a filter threshold and a
/// mana-value constraint, and both must agree).
///
/// Wild Endeavor exercises the search-clause count slot. Its "search your
/// library for a number of basic land cards equal to the other result" clause
/// reaches the die-binding pass through `SearchLibrary`'s own `count`; the
/// intervening `ChangeZone` and `Shuffle` name no die, which is correct — they
/// carry no printed quantity — and are listed with empty expectations so the
/// per-clause reach-guard still accounts for every clause the card lowers to.
#[test]
fn endeavor_cycle_all_roll_two_apportioned_dice() {
    use DieResultSelection::{Chosen, Other};

    for (name, text, sides, expected) in [
        (
            "Arcane Endeavor",
            "Roll two d8 and choose one result. Draw cards equal to that result. Then you may cast an instant or sorcery spell with mana value less than or equal to the other result from your hand without paying its mana cost.",
            8,
            // Draw = chosen; the free cast's filter AND its mana-value
            // constraint both gate on the other die.
            vec![vec![Chosen], vec![Other, Other]],
        ),
        (
            "Grave Endeavor",
            "Roll two d10 and choose one result. Return a creature card from your graveyard to the battlefield with a number of +1/+1 counters on it equal to that result. Then each opponent loses X life and you gain X life, where X is the other result.",
            10,
            // The returned creature's entry counters = chosen. The paired X of
            // "each opponent loses X life AND you gain X life" is ONE printed
            // X, so both halves name the other die.
            vec![vec![Chosen], vec![Other], vec![Other]],
        ),
        (
            "Reckless Endeavor",
            RECKLESS,
            12,
            vec![vec![Chosen], vec![Other]],
        ),
        (
            "Valiant Endeavor",
            "Roll two d6 and choose one result. Destroy each creature with power greater than or equal to that result. Then create a number of 2/2 white Knight creature tokens with vigilance equal to the other result.",
            6,
            // The destroy threshold lives in the population filter, not in an
            // amount slot.
            vec![vec![Chosen], vec![Other]],
        ),
        (
            "Wild Endeavor",
            "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature tokens equal to that result. Then search your library for a number of basic land cards equal to the other result, put them onto the battlefield tapped, then shuffle.",
            4,
            // Token count = chosen; the search's own `count` = other. The
            // trailing ChangeZone and Shuffle carry no printed quantity, so
            // they correctly name no die.
            vec![vec![Chosen], vec![Other], vec![], vec![]],
        ),
    ] {
        let effects = chain(&parse(text, name));
        match effects.first() {
            Some(Effect::RollDie {
                count,
                sides: s,
                selection,
                ..
            }) => {
                assert_eq!(*count, QuantityExpr::Fixed { value: 2 }, "{name}");
                assert_eq!(*s, sides, "{name}");
                // CR 706.4: apportionment is recorded for every cycle member.
                assert_eq!(*selection, Some(2), "{name}");
            }
            other => panic!("{name}: expected leading RollDie, got {other:?}"),
        }

        // Reach-guard: the chain really did lower into the clause count the
        // expectations describe, so the per-clause asserts cannot pass by
        // iterating over a shorter chain than the card prints.
        let post_roll: Vec<&Effect> = effects.iter().skip(1).collect();
        assert_eq!(
            post_roll.len(),
            expected.len(),
            "{name}: expected {} post-roll clauses, got {effects:?}",
            expected.len()
        );

        // CR 706.4: every post-roll clause reads the SPECIFIC die its printed
        // text names, in every quantity slot it carries.
        for (i, (effect, want)) in post_roll.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                &die_selections_of(effect),
                want,
                "{name}: clause [{i}] bound the wrong dice: {effect:?}"
            );
        }
    }
}

/// CR 701.23a: Wild Endeavor's search clause must keep BOTH halves of its
/// printed restriction — the count ("a number of … equal to the other result")
/// and the card filter ("basic land cards").
///
/// The die-binding assertions above see only the count. A search that binds the
/// right die but drops the filter finds one card of ANY type, which is a wrong
/// resolution the apportionment tests cannot detect — and, because the card is
/// otherwise fully parsed, it would be reported as `supported` with zero gaps.
/// This asserts the filter directly so that silent-drop cannot return.
#[test]
fn wild_endeavor_search_keeps_basic_land_filter_and_die_count() {
    let text = "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature \
tokens equal to that result. Then search your library for a number of basic land cards equal to \
the other result, put them onto the battlefield tapped, then shuffle.";

    let effects = chain(&parse(text, "Wild Endeavor"));
    let search = effects
        .iter()
        .find(|e| matches!(e, Effect::SearchLibrary { .. }))
        .unwrap_or_else(|| panic!("expected a SearchLibrary clause, got {effects:?}"));

    let Effect::SearchLibrary { filter, count, .. } = search else {
        unreachable!("filtered to SearchLibrary above");
    };

    // CR 701.23a: the searched-for set is "basic land cards", not "any card".
    let TargetFilter::Typed(typed) = filter else {
        panic!("expected a typed search filter, got {filter:?}");
    };
    assert!(
        typed.type_filters.contains(&TypeFilter::Land),
        "search dropped the Land type filter: {typed:?}"
    );
    assert!(
        typed
            .properties
            .iter()
            .any(|p| matches!(p, FilterProp::HasSupertype { value } if *value == Supertype::Basic)),
        "search dropped the Basic supertype restriction: {typed:?}"
    );

    // CR 706.4: the count is the die the printed text names — "the other result".
    assert_eq!(
        refs_of(count),
        vec![QuantityRef::DieResultSelected {
            selection: DieResultSelection::Other
        }],
        "search count did not bind the other die: {count:?}"
    );
}

/// CR 706.4 + CR 701.23a: every card in the cycle must parse with NO residual
/// diagnostics, which is what the coverage classifier reads to decide whether a
/// card counts as supported.
///
/// This is deliberately a separate guard from the AST assertions above. Those
/// check that the parse is *correct*; this checks that the parser did not also
/// leave an unconsumed text fragment behind while producing that correct AST.
/// Both can diverge: Wild Endeavor's search lowered to exactly the right filter
/// and count while its count phrase ("equal to the other result") was still
/// falling through the search-filter suffix dispatch as an unmatched tail,
/// emitting `TargetFallback`. `check_parse_warnings` treats any such diagnostic
/// as a gap and overrides the card to `supported = false`, so the cycle would
/// have remained an engine gap despite every structural assertion passing.
#[test]
fn endeavor_cycle_parses_without_residual_diagnostics() {
    for (name, text) in [
        (
            "Arcane Endeavor",
            "Roll two d8 and choose one result. Draw cards equal to that result. Then you may cast an instant or sorcery spell with mana value less than or equal to the other result from your hand without paying its mana cost.",
        ),
        (
            "Grave Endeavor",
            "Roll two d10 and choose one result. Return a creature card from your graveyard to the battlefield with a number of +1/+1 counters on it equal to that result. Then each opponent loses X life and you gain X life, where X is the other result.",
        ),
        ("Reckless Endeavor", RECKLESS),
        (
            "Valiant Endeavor",
            "Roll two d6 and choose one result. Destroy each creature with power greater than or equal to that result. Then create a number of 2/2 white Knight creature tokens with vigilance equal to the other result.",
        ),
        (
            "Wild Endeavor",
            "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature tokens equal to that result. Then search your library for a number of basic land cards equal to the other result, put them onto the battlefield tapped, then shuffle.",
        ),
    ] {
        let parsed = parse(text, name);

        // Reach-guard: a card that failed to parse into anything at all would
        // also carry no warnings, so the emptiness assertion below is only
        // meaningful once we know real abilities came out.
        assert!(
            !parsed.abilities.is_empty(),
            "{name}: parsed to no abilities at all"
        );

        assert!(
            parsed.parse_warnings.is_empty(),
            "{name}: parser left residual diagnostics, which the coverage classifier counts as gaps: {:?}",
            parsed.parse_warnings
        );
    }
}

/// CR 706.3b + CR 706.6: cards that roll dice WITHOUT apportioning them are
/// untouched. Berserker's Frenzy is the sharpest neighbour — it rolls TWO dice
/// but IGNORES one (CR 706.6) rather than apportioning both.
#[test]
fn non_apportioned_die_cards_are_unchanged() {
    for (name, text) in [
        (
            "Berserker's Frenzy",
            "Roll two d20 and ignore the lower roll.",
        ),
        ("Six-Sided Die", "Roll a six-sided die."),
        ("Plain multi-roll", "Roll two d6."),
    ] {
        let effects = chain(&parse(text, name));
        for effect in &effects {
            if let Effect::RollDie { selection, .. } = effect {
                // CR 706.6 / CR 706.3b: not an apportionment.
                assert_eq!(*selection, None, "{name} must not be apportioned");
            }
            // Reach-guard for the negative above: no clause may have acquired a
            // die-result binding anywhere in the chain.
            assert!(
                !binds_a_die_result(effect),
                "{name} must not bind a die result"
            );
        }
    }
}

/// CR 608.2c: a demonstrative with NO preceding apportioned roll keeps its
/// ordinary event-context meaning — the rebinding pass must not over-fire.
#[test]
fn demonstrative_without_apportioned_roll_is_untouched() {
    let effects = chain(&parse(
        "Roll a d6. Growth Spurt deals damage equal to that result to target creature.",
        "Single Roll",
    ));
    // Reach-guard: the roll clause really is present and unapportioned.
    assert!(
        matches!(
            effects.first(),
            Some(Effect::RollDie {
                selection: None,
                ..
            })
        ),
        "expected an unapportioned single-die roll: {effects:?}"
    );
    for effect in &effects {
        assert!(
            !binds_a_die_result(effect),
            "a single-die roll must not bind an apportioned die result"
        );
    }
}

/// CR 706.4 + CR 608.2d: the runtime path. The roll suspends for the
/// controller's choice, and after that choice the two clauses consume
/// DIFFERENT dice — the chosen one for damage, the other for Treasures.
///
/// The scenario driver auto-answers `DieResultChoice` by taking index 0
/// (`scenario.rs`), so the CHOSEN die is `results[0]` and "the other result" is
/// `results[1]`. Reading both off the emitted `DieRolled` events is what makes
/// the Treasure count assertion meaningful: if both clauses were wired to the
/// same die, the count would equal the chosen result instead.
#[test]
fn apportioned_roll_feeds_both_clauses_from_different_dice() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // A 1/1 on each side: every d12 result is lethal, so the damage clause is
    // observable no matter which die the driver picks.
    let mine = scenario.add_creature(P0, "Mine", 1, 1).id();
    let theirs = scenario.add_creature(P1, "Theirs", 1, 1).id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Reckless Endeavor", false, RECKLESS)
        .id();
    let mut runner = scenario.build();

    let outcome = runner.cast(spell).resolve();

    // CR 706.1: exactly two dice were rolled, both in range for a d12.
    let rolls: Vec<u8> = outcome
        .events()
        .iter()
        .filter_map(|e| match e {
            GameEvent::DieRolled {
                sides: 12, result, ..
            } => *result,
            _ => None,
        })
        .collect();
    assert_eq!(rolls.len(), 2, "expected two d12 rolls, got {rolls:?}");
    assert!(
        rolls.iter().all(|r| (1..=12).contains(r)),
        "d12 results out of range: {rolls:?}"
    );
    let chosen = i32::from(rolls[0]);
    let other = i32::from(rolls[1]);

    // CR 120.1: "damage equal to that result to each creature" — a d12 result
    // is at least 1, so both 1/1s are destroyed.
    assert_eq!(
        outcome.zone_of(mine),
        Zone::Graveyard,
        "chosen result {chosen} should have destroyed our 1/1"
    );
    assert_eq!(
        outcome.zone_of(theirs),
        Zone::Graveyard,
        "chosen result {chosen} should have destroyed their 1/1"
    );

    // CR 111.1 + CR 706.4: "Treasure tokens equal to the OTHER result" — the
    // die NOT chosen. This is the assertion that fails if both clauses read the
    // same die.
    let treasures = outcome
        .state()
        .objects
        .values()
        .filter(|o| o.zone == Zone::Battlefield && o.name == "Treasure")
        .count();
    assert_eq!(
        treasures, other as usize,
        "expected {other} Treasures (the OTHER die); the chosen die was {chosen}"
    );
}

/// CR 706.4: an apportionment the engine cannot represent is preserved as a
/// GAP, not silently downgraded to a plain roll.
///
/// "the other result" names a unique die only at arity two. A card printing the
/// same apportionment tail on three dice has no single "other", so the roll is
/// left unimplemented rather than lowered to an unapportioned `RollDie` whose
/// later "that result" clauses would fall back to the event-context amount and
/// resolve to 0. Reporting the card as an engine gap is honest; reporting it as
/// supported while it deals 0 damage is not.
#[test]
fn non_binary_apportionment_is_a_gap_not_a_silent_downgrade() {
    let effects = chain(&parse(
        "Roll three d6 and choose one result. Bogus Card deals damage equal to that result to each creature.",
        "Bogus Card",
    ));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Unimplemented { .. })),
        "a non-binary apportionment must surface as a gap: {effects:?}"
    );
    // The specific failure this guards: lowering to a plain unapportioned roll
    // and letting the damage clause quietly read 0.
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            Effect::RollDie {
                selection: None,
                ..
            }
        )),
        "must not downgrade to an unapportioned roll: {effects:?}"
    );
}

/// CR 706.4: the apportionment tail is matched as the phrase cards actually
/// print. Widening it to determiner x noun-number permutations buys no coverage
/// and lets malformed text parse as a real apportionment.
#[test]
fn unprinted_apportionment_permutations_are_not_accepted() {
    for text in [
        "Roll two d6 and choose a result.",
        "Roll two d6 and choose one results.",
    ] {
        let effects = chain(&parse(text, "Bogus Card"));
        for effect in &effects {
            if let Effect::RollDie { selection, .. } = effect {
                assert_eq!(
                    *selection, None,
                    "unprinted phrasing must not parse as an apportionment: {text}"
                );
            }
        }
    }
}

/// CR 706.4: `die_result_this_resolution` and `die_results_apportioned` are
/// resolution-scoped and must not survive the resolution that stamped them.
///
/// The apportioned path resumes through the continuation resumer rather than
/// the end-of-resolution reset in `stack.rs`, so without an explicit clear on
/// the `DieResultChoice` branch it leaked both fields past the boundary. The
/// leak was bounded only incidentally, by the top-of-`apply()` wipe: anything
/// resolving inside the SAME `apply()` window (a trigger that fires off the
/// apportioned spell) would read an apportionment it never rolled. This asserts
/// the boundary directly instead of relying on that containment, and pairs with
/// the `priority_checkpoint_is_settled` clause that now proves both fields gone.
#[test]
fn apportioned_roll_does_not_leak_die_state_past_its_resolution() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.add_creature(P0, "Mine", 1, 1);
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Reckless Endeavor", false, RECKLESS)
        .id();
    let mut runner = scenario.build();

    let outcome = runner.cast(spell).resolve();
    let state = outcome.state();

    assert_eq!(
        state.die_result_this_resolution, None,
        "the chosen die result is resolution-scoped and must be cleared once the \
         apportioned roll's resolution completes"
    );
    assert_eq!(
        state.die_results_apportioned, None,
        "the apportioned (chosen, other) pair is resolution-scoped on the same \
         boundary as the scalar result and must be cleared with it"
    );
}

/// CR 706.4 + CR 608.2d: the apportioned pair must survive a clause that raises
/// its OWN prompt after the die choice.
///
/// Reckless Endeavor is the degenerate shape of this cycle: both die-consuming
/// clauses (`DamageAll`, `Token`) resolve straight through to `Priority` inside
/// a single `apply()`, so the pair never has to cross an action boundary. Every
/// other member of the cycle re-suspends between the choice and the second
/// consumer, and that is the shape the real cards take.
///
/// Arcane Endeavor is the sharpest case: its second clause is a "you may cast"
/// offer, so the chain suspends on `OptionalEffectChoice` after the die choice
/// and the `CastFromZone` clause resolves only on the NEXT action. That clause's
/// gate reads "the other result" via `QuantityRef::DieResultSelected { Other }`,
/// which deliberately has no fallback: an absent apportionment resolves it to 0,
/// so the offered pool collapses to mana value <= 0 and the free cast silently
/// offers nothing.
///
/// The regression this pins: the pair used to be destroyed by the
/// top-of-`apply()` reset before the continuation resumed, so the free cast was
/// wrong across the card's entire legal range while the card was classified as
/// supported with no gaps.
#[test]
fn apportioned_pair_survives_a_clause_that_reprompts() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // Library depth for "draw cards equal to that result" (a d8 draws at most 8).
    scenario.with_library_top(
        P0,
        &["L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9", "L10"],
    );
    // The free-cast pool. Both candidates are instants, so only the mana value
    // gate can exclude either one — that keeps this assertion about "the other
    // result" rather than about card type. Mana value 1 is inside the gate for
    // every possible d8 result (>= 1), so it must ALWAYS be offered; a lost
    // apportionment makes the gate <= 0 and the pool empty.
    let cheap = scenario
        .add_spell_to_hand_from_oracle(P0, "Cheap Instant", true, "Draw a card.")
        .with_mana_cost(ManaCost::generic(1))
        .id();
    // Mana value 9 is outside the gate for every possible d8 result, so it must
    // NEVER be offered. This is the half that fails if the gate were widened
    // instead of fixed — an unbounded pool would include it.
    let expensive = scenario
        .add_spell_to_hand_from_oracle(P0, "Expensive Instant", true, "Draw a card.")
        .with_mana_cost(ManaCost::generic(9))
        .id();
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Arcane Endeavor", false, ARCANE)
        .id();
    let mut runner = scenario.build();

    let mut cast = runner.cast(spell).commit();
    let results = drive_to_die_choice(&mut cast);
    assert_eq!(results.len(), 2, "expected two d8 rolls, got {results:?}");
    let other = i32::from(results[1]);
    cast.act(GameAction::SelectDieResult { index: 0 })
        .expect("selecting a rolled die must be legal");

    // CR 608.2d: the chain re-suspended on the optional cast instead of running
    // to completion. This is the shape Reckless Endeavor cannot reach, and the
    // precondition for the bug.
    assert!(
        matches!(
            cast.state().waiting_for,
            WaitingFor::OptionalEffectChoice { .. }
        ),
        "Arcane Endeavor must re-suspend for its optional cast, got {:?}",
        cast.state().waiting_for
    );

    // Accept the offer — the CastFromZone clause resolves on THIS action, one
    // full action after the die choice that stamped the apportionment.
    cast.act(GameAction::DecideOptionalEffect { accept: true })
        .expect("accepting the optional cast must be legal");

    let WaitingFor::EffectZoneChoice { cards, .. } = cast.state().waiting_for.clone() else {
        panic!(
            "accepting the free cast must offer a pool, got {:?}",
            cast.state().waiting_for
        )
    };

    // CR 706.4: the gate is "mana value <= THE OTHER RESULT", and the other die
    // is at least 1.
    assert!(
        cards.contains(&cheap),
        "the mana value 1 instant must be offered under the other result \
         ({other}); an empty or narrowed pool means the apportioned pair was \
         lost across the action boundary and the gate collapsed to <= 0. \
         Offered: {cards:?}"
    );
    // The other half of the gate: proving the pool is actually bounded by the
    // die, not merely non-empty.
    assert!(
        !cards.contains(&expensive),
        "the mana value 9 instant exceeds every d8 result and must not be \
         offered under the other result ({other}). Offered: {cards:?}"
    );
}

/// CR 706.4 + CR 608.2d: the same boundary crossed through a SEARCH rather than
/// an optional cast — Wild Endeavor searches for "a number of basic land cards
/// equal to the other result", then puts them onto the battlefield tapped.
///
/// A second re-suspension family (`SearchChoice`) reaching the same conclusion
/// is what shows the fix is a property of the resolution boundary itself, not a
/// patch aimed at one card's prompt shape.
///
/// Note where the boundary actually falls for this card, because it is NOT
/// where it falls for Arcane Endeavor. Wild Endeavor raises its `SearchChoice`
/// in the SAME `apply()` as the die choice, so the prompt's `count` is computed
/// before any reset could touch the pair — asserting only that `count` would
/// pass even with the pair destroyed, and would be a vacuous test. What crosses
/// the action boundary here is the search's COMPLETION: the selected lands are
/// moved to the battlefield on the next action, by the continuation that
/// resumes after the pick. So this asserts the post-boundary outcome (the lands
/// actually on the battlefield) as well as the pre-boundary prompt.
#[test]
fn apportioned_pair_survives_a_searching_clause() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    // Enough basics that the search is never capped by library contents — a d4
    // "other result" asks for at most 4.
    scenario.with_library_top(
        P0,
        &["Forest", "Forest", "Forest", "Forest", "Forest", "Forest"],
    );
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Wild Endeavor", false, WILD)
        .id();
    let mut runner = scenario.build();
    make_library_cards_basic_lands(&mut runner, P0, "Forest");

    let mut cast = runner.cast(spell).commit();
    let results = drive_to_die_choice(&mut cast);
    assert_eq!(results.len(), 2, "expected two d4 rolls, got {results:?}");
    let chosen = i32::from(results[0]);
    let other = i32::from(results[1]);
    cast.act(GameAction::SelectDieResult { index: 0 })
        .expect("selecting a rolled die must be legal");

    // CR 701.23a: the search clause suspends for the pick.
    let WaitingFor::SearchChoice { cards, count, .. } = cast.state().waiting_for.clone() else {
        panic!(
            "Wild Endeavor must suspend for its library search, got {:?}",
            cast.state().waiting_for
        )
    };

    // CR 706.4: "a number of basic land cards equal to THE OTHER RESULT".
    assert_eq!(
        count, other as usize,
        "the search must ask for the other result ({other}) basics, not {count}"
    );
    // Guard the mirror failure: reading the CHOSEN die would also be wrong.
    // Only discriminating when the two dice actually differ.
    if chosen != other {
        assert_ne!(
            count, chosen as usize,
            "the search must read the OTHER die ({other}), not the chosen one ({chosen})"
        );
    }

    // Complete the search. The selected lands reach the battlefield on THIS
    // action, in the continuation that resumes after the pick — the far side of
    // the boundary that the prompt's `count` never crossed.
    let picked: Vec<ObjectId> = cards.into_iter().take(count).collect();
    cast.act(GameAction::SelectCards {
        cards: picked.clone(),
    })
    .expect("submitting the searched basics must be legal");

    // CR 706.4 + CR 701.23d: every searched land actually arrived, and the
    // count that arrived is still "the other result".
    let on_battlefield = picked
        .iter()
        .filter(|id| {
            cast.state()
                .objects
                .get(id)
                .is_some_and(|o| o.zone == Zone::Battlefield)
        })
        .count();
    assert_eq!(
        on_battlefield,
        other as usize,
        "all {other} searched basics (the other result) must reach the battlefield;          got {on_battlefield}"
    );
}

/// CR 706.4: preserving the pair across action boundaries must not turn it into
/// a leak — it still dies with the resolution that rolled it.
///
/// `apportioned_roll_does_not_leak_die_state_past_its_resolution` proves this
/// for the straight-through shape. The preservation added for the re-suspending
/// shape widens exactly the window that test closes, so the re-suspending shape
/// needs its own proof that the clear still lands: the action that ENDS such a
/// resolution is a later action than the one that made the choice, so the clear
/// cannot live on the die-choice branch.
#[test]
fn apportioned_pair_is_cleared_after_a_reprompting_resolution_completes() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(
        P0,
        &["L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9", "L10"],
    );
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Arcane Endeavor", false, ARCANE)
        .id();
    let mut runner = scenario.build();

    let mut cast = runner.cast(spell).commit();
    drive_to_die_choice(&mut cast);
    cast.act(GameAction::SelectDieResult { index: 0 })
        .expect("selecting a rolled die must be legal");
    // Decline the optional cast, which completes the resolution on this action.
    cast.act(GameAction::DecideOptionalEffect { accept: false })
        .expect("declining the optional cast must be legal");

    let state = cast.state();
    assert!(
        matches!(state.waiting_for, WaitingFor::Priority { .. }) && state.stack.is_empty(),
        "declining the optional cast must complete the resolution, got {:?}",
        state.waiting_for
    );
    assert_eq!(
        state.die_results_apportioned, None,
        "the apportioned pair is preserved only WHILE its resolution is suspended; \
         once that resolution completes it must be cleared, or the next roll-less \
         resolution would read an apportionment it never rolled"
    );
    assert_eq!(
        state.die_result_this_resolution, None,
        "the scalar die result is cleared on the same boundary as the pair"
    );
}

/// CR 706.4 + CR 608.2d: the boundary invariant itself, stated once and
/// independently of any card's prompt shape.
///
/// The two card-driven tests above each cross the boundary through one specific
/// prompt family, and they are the proof that real cards work. This one asserts
/// the underlying rule they both depend on — that an action submitted WHILE a
/// resolution is suspended preserves the apportionment, and the action that
/// ENDS the resolution clears it — so the property stays pinned even if a
/// future card's clause ordering changes which prompt happens to appear.
///
/// The distinction matters because "the pair survives" and "the pair is
/// cleared" are opposite failure directions, and a fix for either one alone is
/// easy to write and wrong: preserving unconditionally leaks the pair into the
/// next resolution, and clearing on the die-choice action destroys it before a
/// re-suspending chain's second consumer reads it.
#[test]
fn apportionment_is_preserved_while_suspended_and_cleared_at_resolution_end() {
    let mut scenario = GameScenario::new();
    scenario.at_phase(Phase::PreCombatMain);
    scenario.with_library_top(
        P0,
        &["L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9", "L10"],
    );
    scenario
        .add_spell_to_hand_from_oracle(P0, "Cheap Instant", true, "Draw a card.")
        .with_mana_cost(ManaCost::generic(1));
    let spell = scenario
        .add_spell_to_hand_from_oracle(P0, "Arcane Endeavor", false, ARCANE)
        .id();
    let mut runner = scenario.build();

    let mut cast = runner.cast(spell).commit();
    let results = drive_to_die_choice(&mut cast);
    cast.act(GameAction::SelectDieResult { index: 0 })
        .expect("selecting a rolled die must be legal");

    let expected = (i32::from(results[0]), i32::from(results[1]));

    // Suspended: the pair is intact and matches the dice actually rolled.
    assert!(
        !matches!(cast.state().waiting_for, WaitingFor::Priority { .. }),
        "precondition: the chain must still be suspended here, got {:?}",
        cast.state().waiting_for
    );
    assert_eq!(
        cast.state().die_results_apportioned,
        Some(expected),
        "while the resolution is suspended the apportionment must be preserved          exactly as rolled (chosen, other)"
    );

    // Cross one more action boundary while still suspended. This is the exact
    // step that used to destroy the pair.
    cast.act(GameAction::DecideOptionalEffect { accept: true })
        .expect("accepting the optional cast must be legal");
    assert_eq!(
        cast.state().die_results_apportioned,
        Some(expected),
        "the apportionment must survive EVERY action taken while its resolution          is still suspended, not merely the first"
    );

    // Decline the offered cast, ending the resolution.
    cast.act(GameAction::SelectCards { cards: vec![] })
        .expect("declining the offered free cast must be legal");
    assert!(
        matches!(cast.state().waiting_for, WaitingFor::Priority { .. })
            && cast.state().stack.is_empty(),
        "declining the free cast must end the resolution, got {:?}",
        cast.state().waiting_for
    );
    assert_eq!(
        cast.state().die_results_apportioned, None,
        "once the resolution ends the apportionment must be gone, so an unrelated          later resolution cannot read dice it never rolled"
    );
}
