use crate::types::ability::{
    ControllerRef, DelayedTriggerCondition, DelayedTriggerLifetime, Effect, EffectError,
    EffectKind, ResolvedAbility, TargetFilter, TargetRef, TriggerDefinition,
};
use crate::types::events::GameEvent;
use crate::types::game_state::{DelayedTrigger, GameState};
use crate::types::player::PlayerId;

/// Digital-only Alchemy (no CR entry): "you get a one-time boon with
/// `<ability>`" installs the granted trigger for the recipient as a
/// Persistent one-shot `WhenNextEvent` delayed trigger. One-shot removal,
/// intervening-if gating, cross-turn persistence, and cleanup survival all
/// reuse the CR 603.7 machinery; `DelayedTrigger::is_boon` distinguishes the
/// entry for "if you have a boon" (`TriggerCondition::HasBoon`) and for the
/// boon-only embedded-condition gate in delayed matching.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let (recipient, inner) = match &ability.effect {
        Effect::CreateBoon { recipient, trigger } => (recipient.clone(), trigger.as_ref().clone()),
        _ => {
            return Err(EffectError::MissingParam("CreateBoon".to_string()));
        }
    };
    let holder = resolve_recipient(state, ability, &recipient)?;
    let execute = inner.execute.clone().ok_or_else(|| {
        EffectError::MissingParam("boon inner trigger has no execute".to_string())
    })?;

    // CR 603.7a: the delayed condition carries the event matcher; the effect
    // lives in the delayed ability, not in the embedded trigger.
    let mut embedded = inner;
    embedded.execute = None;
    reanchor_holder_refs(&mut embedded, holder);

    let condition = DelayedTriggerCondition::WhenNextEvent {
        trigger: Box::new(embedded),
        or_trigger: None,
        lifetime: DelayedTriggerLifetime::Persistent,
    };

    // CR 603.7c + CR 608.2c: same forwarded-result rebind as
    // `delayed_trigger::resolve` — a forward-result continuation can
    // temporarily rebind `source_id`, but the boon remains owned by the
    // trigger source, which the captured provenance preserves.
    let delayed_source_id = ability
        .context
        .forwarded_result_context
        .as_ref()
        .and(ability.trigger_source.as_ref())
        .map(|source| source.identity.reference.object_id)
        .unwrap_or(ability.source_id);
    // CR 608.2h: propagate the creating ability's chain-root target list (see
    // `build_resolved_from_def_with_chain_root`'s doc). The delayed ability
    // is controlled by the HOLDER, not the creator, so every "you" inside
    // the granted body reads the holder.
    let mut delayed_ability = crate::game::ability_utils::build_resolved_from_def_with_chain_root(
        &execute,
        delayed_source_id,
        holder,
        ability.context.chain_root_targets.clone(),
    );

    // Same source-context propagation as `delayed_trigger::resolve`: matchers
    // read the source snapshot for event attribution, and the boon's source
    // is the creating object.
    let source_context = ability.trigger_source.clone().or_else(|| {
        state
            .objects
            .get(&ability.source_id)
            .map(|source| super::super::triggers::trigger_source_context_for_latch(state, source))
    });
    if let Some(mut source_context) = source_context {
        if source_context.identity.reference.object_id == delayed_ability.source_id {
            if let Some(obj) = state.objects.get(&delayed_ability.source_id) {
                source_context.identity.expected_zone = obj.zone;
                source_context.identity.reference.incarnation = obj.incarnation;
            }
        }
        delayed_ability.set_trigger_source_recursive(source_context);
    }
    // Digital-only Alchemy (no CR entry): the source-context setter stamps
    // the CREATOR as controller (correct for ordinary delayed triggers).
    // A boon belongs to its HOLDER, so re-stamp after — otherwise "you" in
    // the granted body would read the creator when they differ (Loch
    // Larent, Valiant Batrider).
    delayed_ability.set_controller_recursive(holder);

    // CR 701.27f + CR 400.7: same creation-time generation/incarnation
    // capture as `delayed_trigger::resolve`.
    let source = state
        .objects
        .get(&ability.source_id)
        .filter(|object| object.back_face.is_some());
    let source_transformation_count = source.map(|object| object.transformation_count);
    delayed_ability.set_source_transformation_count_recursive(source_transformation_count);
    delayed_ability.set_source_incarnation_recursive(source.map(|object| object.incarnation));

    crate::game::triggers::install_delayed_trigger(
        state,
        DelayedTrigger {
            condition,
            ability: Box::new(delayed_ability),
            controller: holder,
            source_id: delayed_source_id,
            one_shot: true,
            is_boon: true,
            provenance: crate::types::identifiers::DelayedInstallIdentity::LegacyDelayed,
        },
        events,
    );

    events.push(GameEvent::EffectResolved {
        kind: EffectKind::CreateBoon,
        source_id: ability.source_id,
        subject: None,
    });

    Ok(())
}

/// Lower the parser-emitted recipient to the concrete boon holder.
///
/// * `Controller` ("you get") is the resolving ability's controller.
/// * `TriggeringPlayer` ("that player gets", Valiant Batrider) is the player
///   of the event that fired the creating trigger, read live from
///   `current_trigger_event`.
/// * A declarative player filter (`Typed`, `Player`, `Opponent` — Loch
///   Larent's "target opponent") or `ParentTarget` reads the first chosen
///   player target, which chain propagation in
///   `effects::mod.rs::resolve_ability_chain` copied onto this sub-ability.
/// * `SpecificPlayer` is already concrete.
fn resolve_recipient(
    state: &GameState,
    ability: &ResolvedAbility,
    recipient: &TargetFilter,
) -> Result<PlayerId, EffectError> {
    match recipient {
        TargetFilter::Controller => Ok(ability.controller),
        TargetFilter::SpecificPlayer { id } => Ok(*id),
        TargetFilter::TriggeringPlayer => state
            .current_trigger_event
            .as_ref()
            .and_then(|event| crate::game::targeting::extract_player_from_event(event, state))
            .ok_or_else(|| {
                EffectError::MissingParam("boon recipient has no triggering player".to_string())
            }),
        TargetFilter::ParentTarget
        | TargetFilter::Typed(_)
        | TargetFilter::Player
        | TargetFilter::Opponent => ability
            .targets
            .iter()
            .find_map(|target| match target {
                TargetRef::Player(id) => Some(*id),
                _ => None,
            })
            .ok_or_else(|| {
                EffectError::InvalidParam("boon recipient has no chosen player".to_string())
            }),
        _ => Err(EffectError::InvalidParam(
            "unsupported boon recipient".to_string(),
        )),
    }
}

/// Re-anchor the embedded matcher's holder-relative references to the
/// concrete holder.
///
/// Delayed-trigger matchers resolve "you" through the trigger source's
/// controller, which is the boon's CREATOR. When the holder differs from the
/// creator (Loch Larent, Valiant Batrider), an un-rewritten `Controller`
/// would watch the wrong player's events. Binding unconditionally (even when
/// holder == creator) additionally immunizes self-held boons against the
/// creating object changing controller mid-flight; in the common case the
/// bound filter matches exactly what the relative one would.
///
/// Only `You`-shaped references are re-anchored. `Opponent` and the other
/// relative `ControllerRef`s have no concrete singular form and never appear
/// in a printed boon inner; execute-time "you" needs no rewrite because the
/// delayed ability is already controlled by the holder.
fn reanchor_holder_refs(inner: &mut TriggerDefinition, holder: PlayerId) {
    if matches!(inner.valid_target, Some(TargetFilter::Controller)) {
        inner.valid_target = Some(TargetFilter::SpecificPlayer { id: holder });
    }
    for slot in [
        inner.valid_card.as_mut(),
        inner.valid_source.as_mut(),
        inner.valid_target.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        reanchor_filter(slot, holder);
    }
}

fn reanchor_filter(filter: &mut TargetFilter, holder: PlayerId) {
    match filter {
        TargetFilter::Controller => {
            *filter = TargetFilter::SpecificPlayer { id: holder };
        }
        TargetFilter::Typed(typed) => {
            if matches!(typed.controller, Some(ControllerRef::You)) {
                typed.controller = Some(ControllerRef::SpecificPlayer { id: holder });
            }
        }
        TargetFilter::Not { filter } => reanchor_filter(filter, holder),
        TargetFilter::Or { filters } | TargetFilter::And { filters } => {
            for nested in filters {
                reanchor_filter(nested, holder);
            }
        }
        _ => {}
    }
}
