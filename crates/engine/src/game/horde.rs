//! Horde Magic — the self-piloting "Horde" seat's scripted turn runtime.
//!
//! Horde Magic is a casual cooperative variant (not DCI/CR-sanctioned) in which
//! a team of survivors faces a single automated "Horde" deck. The Horde reuses
//! CR mechanisms the engine already models: the one-vs-many topology of
//! Archenemy (CR 904) with the Horde in the archenemy seat, and shared team
//! turns (CR 805) for the survivor team. This module is the runtime sibling of
//! [`crate::game::archenemy`] / [`crate::game::planechase`]: it owns the Horde's
//! precombat-main "reveal and resolve" wave.
//!
//! ## Reveal-and-resolve wave
//!
//! At the start of the Horde's precombat main phase the Horde reveals cards from
//! the top of its library and either casts them (nontoken cards, for free — the
//! Cascade/Discover free-cast authority, CR 608.2g) or puts them onto the
//! battlefield (tokens — CR 111; deferred to a later PR). A "wave" reveals a
//! ruleset-defined number of cards ([`WaveTermination`]).
//!
//! Because each free cast sets [`GameState::waiting_for`] and puts one spell on
//! the stack (the spell must then resolve through the normal priority/stack
//! loop), a wave cannot cast N cards in a single synchronous call. Instead:
//!
//!  - [`begin_wave`] runs as the Horde's precombat main begins (dispatched from
//!    `turns::finish_enter_phase`, mutually exclusive with the archenemy
//!    `set_in_motion` turn-based action) and seeds
//!    [`GameState::horde_wave_remaining`] with the wave size. It does NOT cast —
//!    setting `waiting_for` inside `finish_enter_phase` would be clobbered by the
//!    phase executor's own priority grant.
//!  - [`maybe_reveal_next`] reveals-and-resolves exactly ONE card and decrements
//!    the counter. It is invoked (a) from the `PreCombatMain` arm of
//!    `turns::auto_advance` for the first card (mirroring the Paradigm
//!    turn-based-action hook), and (b) from the priority-grant seam in
//!    `engine::pass_priority_once_with_pipeline` after each Horde spell resolves
//!    and priority returns to the Horde with an empty stack, until the counter
//!    reaches zero.

use crate::types::ability::{
    CardPlayMode, CastFromZoneDriver, Effect, ResolvedAbility, TargetFilter, TargetRef,
};
use crate::types::events::GameEvent;
use crate::types::format::{GameFormat, WaveTermination};
use crate::types::game_state::{GameState, WaitingFor};
use crate::types::identifiers::ObjectId;
use crate::types::phase::Phase;
use crate::types::player::PlayerId;
use crate::types::zones::Zone;

/// The Horde seat, if this is a Horde game. Live read via the topology accessor
/// (single authority, CR 904.2a-style), never a latched copy.
pub(crate) fn horde_seat(state: &GameState) -> Option<PlayerId> {
    if state.format_config.format != GameFormat::Horde {
        return None;
    }
    crate::game::topology::archenemy(state)
}

/// True when it is currently the Horde's turn — the active player is the Horde
/// seat in a Horde game. Used to gate the draw-step skip and the wave.
pub(crate) fn is_horde_turn(state: &GameState) -> bool {
    horde_seat(state) == Some(state.active_player)
}

/// How many cards the Horde reveals-and-resolves this wave. PR2 implements only
/// the base `FixedCount(n)` policy; live-state bonuses (one extra per Horde
/// artifact, one extra per additional survivor) and the token-heavy
/// `UntilNonToken` policy are later PRs.
fn wave_count(state: &GameState) -> u32 {
    match state.format_config.horde_ruleset.as_ref().map(|r| &r.wave) {
        Some(WaveTermination::FixedCount(n)) => *n,
        None => 0,
    }
}

/// Seed the Horde's precombat-main wave counter. Called from
/// `turns::finish_enter_phase` as the Horde's precombat main begins, taking the
/// place of the (no-op) archenemy `set_in_motion` turn-based action for a Horde
/// game. This only sets the counter; the actual reveal-and-resolve is driven one
/// card at a time by [`maybe_reveal_next`] (see the module docs for why the cast
/// cannot happen here).
pub(crate) fn begin_wave(state: &mut GameState, _events: &mut [GameEvent]) {
    if !is_horde_turn(state) {
        return;
    }
    state.horde_wave_remaining = wave_count(state);
}

/// If a Horde wave is in progress and the Horde is at an empty-stack priority
/// window during its precombat main, reveal-and-resolve the next card and
/// decrement the wave counter. Returns `Some(new_waiting_for)` when a card was
/// revealed (the caller must adopt it), or `None` when the wave is not eligible
/// to advance this beat (not the Horde's turn, wrong phase, non-empty stack,
/// counter exhausted, or an empty library).
pub(crate) fn maybe_reveal_next(
    state: &mut GameState,
    events: &mut Vec<GameEvent>,
) -> Option<WaitingFor> {
    // Gate: only advance the wave when the Horde is the one about to act, its
    // precombat main is open, the stack is clear (the previous card resolved),
    // and cards remain to reveal.
    if state.horde_wave_remaining == 0
        || state.phase != Phase::PreCombatMain
        || !state.stack.is_empty()
        || !is_horde_turn(state)
    {
        return None;
    }
    let horde = horde_seat(state)?;

    // Reveal the top card of the Horde's library.
    let Some(card_id) = state
        .players
        .iter()
        .find(|p| p.id == horde)
        .and_then(|p| p.library.front().copied())
    else {
        // Empty library: nothing more to reveal this wave. The Horde loses by
        // decking out (a later PR); here we simply end the wave cleanly.
        state.horde_wave_remaining = 0;
        return None;
    };

    // Consume one from the wave budget regardless of the branch taken so the
    // wave always terminates.
    state.horde_wave_remaining = state.horde_wave_remaining.saturating_sub(1);

    let is_token = state.objects.get(&card_id).is_some_and(|obj| obj.is_token);

    if is_token {
        // TODO(PR4 — token-in-library primitive): put a fresh battlefield token
        // (from the revealed object's stored `TokenCharacteristics`) under the
        // Horde's control and remove the library object. Until that lands, move
        // the token out of the library so the wave advances instead of spinning
        // on the same card; the cease-to-exist SBA (CR 111.8) then sweeps it.
        // The PR2 fixture contains no library tokens, so this branch is inert.
        crate::game::zones::move_to_zone(state, card_id, Zone::Graveyard, events);
        return None;
    }

    // Nontoken: cast it for free during resolution. Mirror Cascade — move the
    // card to exile first (CR 601.2a: the reveal), then drive the free cast
    // through the single free-cast authority (`cast_from_zone::resolve`'s
    // `driver_free_cast` gate, CR 608.2g), which casts from the card's current
    // (exile) zone at zero cost. X-cost nontokens resolve at X = 0 (CR 601.2b),
    // exactly like Cascade/Discover.
    crate::game::zones::move_to_zone(state, card_id, Zone::Exile, events);
    if state.objects.get(&card_id).map(|o| o.zone) != Some(Zone::Exile) {
        // A replacement effect redirected the reveal; do not attempt to cast.
        return None;
    }

    let ability = free_cast_ability(card_id, horde);
    match crate::game::effects::cast_from_zone::resolve(state, &ability, events) {
        Ok(()) => Some(state.waiting_for.clone()),
        Err(_) => None,
    }
}

/// Build the synthetic `Effect::CastFromZone` that free-casts a single revealed
/// nontoken card during resolution (the Cascade/Discover authority). The card is
/// supplied as the sole explicit target so `cast_from_zone::resolve` routes it to
/// the `driver_free_cast` gate. The Horde is the controller so the spell resolves
/// (and any permanent enters) under the Horde's control.
fn free_cast_ability(card_id: ObjectId, horde: PlayerId) -> ResolvedAbility {
    ResolvedAbility::new(
        Effect::CastFromZone {
            target: TargetFilter::Any,
            without_paying_mana_cost: true,
            mode: CardPlayMode::Cast,
            cast_transformed: false,
            alt_ability_cost: None,
            constraint: None,
            duration: None,
            driver: CastFromZoneDriver::DuringResolution,
            mana_spend_permission: None,
        },
        vec![TargetRef::Object(card_id)],
        card_id,
        horde,
    )
}
