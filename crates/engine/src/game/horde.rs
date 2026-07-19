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
//! battlefield (tokens — CR 111: a token is never cast, so a revealed library
//! token enters directly via [`reveal_library_token`]). A "wave" reveals a
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

use crate::game::effects::token::apply_create_token_after_replacement;
use crate::game::replacement::{self, ReplacementResult};
use crate::types::ability::{
    CardPlayMode, CastFromZoneDriver, Effect, ResolvedAbility, TargetFilter, TargetRef,
};
use crate::types::card::Rarity;
use crate::types::events::GameEvent;
use crate::types::format::{GameFormat, WaveTermination};
use crate::types::game_state::{GameState, WaitingFor};
use crate::types::identifiers::ObjectId;
use crate::types::phase::Phase;
use crate::types::player::PlayerId;
use crate::types::proposed_event::{EtbTapState, ProposedEvent, TokenCharacteristics, TokenSpec};
use crate::types::zones::Zone;
use std::collections::HashSet;

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

/// True when `id` is the Horde seat, which in this casual variant has no life
/// total. Damage/life loss it would suffer is redirected to milling (see
/// `effects::life`), and it is exempt from the CR 704.5a "0 or less life loses"
/// state-based action (`sba::collect_life_losers`). This is not a
/// CR-sanctioned rule — Horde Magic is a casual format — so the helper names
/// the *mechanism* (a seat with no life total) rather than citing a fictional
/// rule number.
pub(crate) fn player_has_no_life_total(state: &GameState, id: PlayerId) -> bool {
    horde_seat(state) == Some(id)
}

/// The Horde is defeated (and the survivors win) when its library is empty AND
/// it controls no creature on the battlefield. This is the Horde-variant loss
/// condition consumed by `elimination::check_game_over` in place of the generic
/// archenemy "still living" check — the Horde has no life total, so it can never
/// be eliminated by the ordinary life/poison state-based actions. Casual-format
/// rule (no CR number); it stands in for the archenemy-alive predicate of
/// CR 104.2a's win check.
pub(crate) fn horde_is_defeated(state: &GameState) -> bool {
    let Some(horde) = horde_seat(state) else {
        return false;
    };
    let library_empty = state
        .players
        .iter()
        .find(|p| p.id == horde)
        .is_none_or(|p| p.library.is_empty());
    let controls_creature = state.battlefield.iter().any(|id| {
        state.objects.get(id).is_some_and(|obj| {
            obj.controller == horde
                && obj
                    .card_types
                    .core_types
                    .contains(&crate::types::card_type::CoreType::Creature)
        })
    });
    library_empty && !controls_creature
}

/// The wave policy for this Horde game, if any.
fn wave_policy(state: &GameState) -> Option<WaveTermination> {
    state.format_config.horde_ruleset.as_ref().map(|r| r.wave)
}

/// Seed value for the Horde's precombat-main wave counter.
///
/// - `FixedCount(n)`: the wave reveals exactly `n` cards, so seed `n`.
/// - `UntilNonToken` / `UntilRarityAtLeast(_)`: the wave reveals until a
///   terminating card resolves (the first non-token, or the first card at/above
///   a rarity threshold — see [`maybe_reveal_next`]). The counter is only a
///   *safety bound* for these — seed it with the Horde's current library size so
///   the loop still terminates if the library empties before the terminating
///   card appears.
///
/// Live-state bonuses (one extra per Horde artifact, one extra per additional
/// survivor) are a later PR.
fn wave_seed(state: &GameState) -> u32 {
    match wave_policy(state) {
        Some(WaveTermination::FixedCount(n)) => n,
        Some(WaveTermination::UntilNonToken { .. })
        | Some(WaveTermination::UntilRarityAtLeast(_)) => horde_seat(state)
            .and_then(|horde| state.players.iter().find(|p| p.id == horde))
            .map_or(0, |p| p.library.len() as u32),
        None => 0,
    }
}

/// How many NON-token cards must resolve to end this Horde turn's wave.
///
/// Only meaningful for `UntilNonToken`; every other policy terminates on a
/// different axis (a fixed card count, or a rarity threshold) and reports `0`.
/// The count is resolved against `turn_index` because a schedule may vary per
/// turn — `WaveCount::Snaking` ramps up and back down.
fn wave_nontoken_seed(state: &GameState, turn_index: u32) -> u32 {
    match wave_policy(state) {
        Some(WaveTermination::UntilNonToken { count }) => count.nontokens_for_turn(turn_index),
        _ => 0,
    }
}

/// Whether revealing-and-casting a NON-TOKEN card of the given rarity ends the
/// wave under `policy`.
///
/// - `UntilNonToken { .. }`: ends once the wave's required number of nontokens
///   have resolved — i.e. when the remaining count reaches zero AFTER this card
///   is accounted for. `nontokens_remaining_after` is that post-decrement value.
/// - `UntilRarityAtLeast(t)`: only a card whose rarity is at least `t` ends it; a
///   below-threshold (common) card is cast and the wave CONTINUES, exactly like
///   `FixedCount` — the per-card decrement in [`maybe_reveal_next`] already
///   accounts for it.
/// - `FixedCount` / no policy: never ends the wave here; the counter alone
///   governs.
///
/// A card of unknown rarity (`None`) never ends a rarity wave, so a card-data gap
/// fails safe toward "keep revealing" rather than stalling the Horde on turn one.
///
/// Split out as a pure function so the whole policy matrix is testable without
/// driving a free cast through the stack.
fn wave_ends_after_nontoken(
    policy: Option<WaveTermination>,
    rarity: Option<Rarity>,
    nontokens_remaining_after: u32,
) -> bool {
    match policy {
        Some(WaveTermination::UntilNonToken { .. }) => nontokens_remaining_after == 0,
        Some(WaveTermination::UntilRarityAtLeast(threshold)) => {
            rarity.is_some_and(|r| r >= threshold)
        }
        Some(WaveTermination::FixedCount(_)) | None => false,
    }
}

/// Seed the Horde's precombat-main wave counters. Called from
/// `turns::finish_enter_phase` as the Horde's precombat main begins, taking the
/// place of the (no-op) archenemy `set_in_motion` turn-based action for a Horde
/// game. This only sets the counters; the actual reveal-and-resolve is driven one
/// card at a time by [`maybe_reveal_next`] (see the module docs for why the cast
/// cannot happen here).
///
/// The Horde turn index advances here (and only here), so a per-turn wave
/// schedule (`WaveCount::Snaking`) reads a stable index for the whole wave.
pub(crate) fn begin_wave(state: &mut GameState, _events: &mut [GameEvent]) {
    if !is_horde_turn(state) {
        return;
    }
    let turn_index = state.horde_turn_index;
    state.horde_wave_remaining = wave_seed(state);
    state.horde_wave_nontokens_remaining = wave_nontoken_seed(state, turn_index);
    // Advance for the NEXT Horde turn; this turn's wave keeps `turn_index`.
    state.horde_turn_index = turn_index.saturating_add(1);
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

    // Reveal-and-resolve cards until either the wave pauses on a nontoken spell
    // (which is put on the stack and resolved through the normal priority loop
    // before the `engine::pass_priority_once` re-entry seam re-invokes this
    // function) or the wave budget / library is exhausted.
    //
    // Tokens must be handled in a LOOP here rather than one-per-beat: a token is
    // never cast (CR 111) and enters the battlefield synchronously, so it never
    // goes on the stack and cannot pause the wave. The re-entry seam only fires
    // after a Horde SPELL resolves and priority returns to the Horde with the
    // stack becoming empty; a token that returned `None` (stack already empty,
    // `waiting_for` unchanged) would strand every later card in the wave. So on
    // a token we resolve its ETB and immediately `continue` to the next card.
    loop {
        // Wave budget exhausted (e.g. after the last card was a token that
        // decremented the counter to zero) — stop cleanly.
        if state.horde_wave_remaining == 0 {
            return None;
        }

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
            // CR 111 + CR 111.1: a token can never be cast, so a revealed library
            // token is put directly onto the battlefield under the Horde's
            // control. Enters synchronously (no stack), so continue the wave with
            // the next card in this same call.
            reveal_library_token(state, card_id, horde, events);
            continue;
        }

        // Capture the card's library rarity BEFORE it leaves the library, for the
        // `UntilRarityAtLeast` end-of-wave check below. (The object survives the
        // exile + free-cast, but reading it here keeps the rarity next to the
        // reveal and independent of what the cast does to the object.)
        let revealed_rarity = state
            .objects
            .get(&card_id)
            .and_then(|o| o.horde_library_rarity);

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
        return match crate::game::effects::cast_from_zone::resolve(state, &ability, events) {
            Ok(()) => {
                // This card is a resolved NON-token, so it counts against an
                // `UntilNonToken` wave's quota (saturating: other policies leave
                // the quota at 0 and are unaffected).
                state.horde_wave_nontokens_remaining =
                    state.horde_wave_nontokens_remaining.saturating_sub(1);

                // Ending the wave clears the counter so the priority re-entry seam
                // (which re-invokes this function once the spell resolves) reveals
                // nothing more, leaving the rest of the library for the next Horde
                // turn. See [`wave_ends_after_nontoken`] for the policy matrix.
                if wave_ends_after_nontoken(
                    wave_policy(state),
                    revealed_rarity,
                    state.horde_wave_nontokens_remaining,
                ) {
                    state.horde_wave_remaining = 0;
                }
                Some(state.waiting_for.clone())
            }
            Err(_) => None,
        };
    }
}

/// Reveal a token from the top of the Horde's library onto the battlefield.
///
/// CR 111: a token can never be cast, so a token in the Horde's library is put
/// directly onto the battlefield when revealed. A FRESH battlefield token is
/// created (CR 111.1 + CR 111.2 — the Horde is the token's owner and its
/// controller) from the library object's stored body characteristics, routed
/// through the ordinary token-creation replacement pipeline exactly like any
/// other created token — so ETB triggers fire and the Horde emblem's haste +
/// must-attack apply. The original library placeholder object is then removed so
/// no duplicate remains (the M2 "create + remove" lifecycle, chosen over a
/// literal library→battlefield move so the shared CR 111.8 movement guards stay
/// untouched). The fresh battlefield token does NOT carry `in_horde_library`
/// (it is a normal token via `create_object`), so it will cease to exist
/// normally (CR 704.5d) if it later leaves the battlefield.
fn reveal_library_token(
    state: &mut GameState,
    card_id: ObjectId,
    horde: PlayerId,
    events: &mut Vec<GameEvent>,
) {
    let Some(obj) = state.objects.get(&card_id) else {
        return;
    };
    // Reconstruct the token body from the revealed library object (mirrors the
    // GameObject → TokenCharacteristics projection used by `token_copy`). A Horde
    // library token is materialized from a `TokenCharacteristics` body, so this
    // projection round-trips its full identity. `TokenCharacteristics` carries no
    // channel for non-keyword activated/triggered/static abilities — the token
    // body model only encodes P/T, types, colors, and keywords — so the created
    // token carries exactly those (`static_abilities` empty). If a future Horde
    // token needs granted statics, thread them through `TokenSpec::static_abilities`.
    let characteristics = TokenCharacteristics {
        display_name: obj.name.clone(),
        power: obj.power,
        toughness: obj.toughness,
        core_types: obj.card_types.core_types.clone(),
        subtypes: obj.card_types.subtypes.clone(),
        supertypes: obj.card_types.supertypes.clone(),
        colors: obj.color.clone(),
        keywords: obj.keywords.clone(),
    };
    let spec = TokenSpec {
        characteristics,
        script_name: obj.name.clone(),
        static_abilities: Vec::new(),
        enter_with_counters: Vec::new(),
        tapped: false,
        enters_attacking: false,
        sacrifice_at: None,
        // Token-creation source (plumbing, not a rule): there is no card/ability
        // source for a Horde reveal (it is a turn-based action), so the library
        // placeholder — still present at apply time; removed just below — stands
        // in as the source for the token-art lookup. No delayed trigger or
        // `enters_attacking` references it, so its removal leaves nothing dangling.
        source_id: card_id,
        controller: horde,
        attach_to: None,
    };
    let proposed = ProposedEvent::CreateToken {
        owner: horde,
        spec: Box::new(spec),
        copy: None,
        enter_tapped: EtbTapState::Unspecified,
        count: 1,
        applied: HashSet::new(),
    };
    // CR 614.16: token-creation replacement effects apply to the revealed token.
    match replacement::replace_event(state, proposed, events) {
        ReplacementResult::Execute(event) => {
            apply_create_token_after_replacement(state, event, events);
        }
        // The Horde's own token creation is not subject to a player's opt-in
        // replacement choice; treat prevention / (unreachable) choice as no ETB.
        ReplacementResult::Prevented | ReplacementResult::NeedsChoice(_) => {}
    }
    // Remove the library placeholder so no duplicate token remains.
    crate::game::zones::remove_from_zone(state, card_id, Zone::Library, horde);
    state.objects.remove(&card_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::format::WaveCount;

    /// The rarity-wave gate is a `>=` comparison, so the `Ord` derive on
    /// [`Rarity`] must order least-rare first. Pin it: if the enum is ever
    /// reordered, every `UntilRarityAtLeast` threshold silently changes meaning.
    #[test]
    fn rarity_orders_least_rare_first() {
        assert!(Rarity::Common < Rarity::Uncommon);
        assert!(Rarity::Uncommon < Rarity::Rare);
        assert!(Rarity::Rare < Rarity::Mythic);
    }

    /// The D&D Horde's rule: "a wave ends when an Uncommon, Rare, or Mythic
    /// enters" — commons keep it going, uncommon-or-better caps it.
    #[test]
    fn rarity_wave_ends_only_at_or_above_the_threshold() {
        let policy = Some(WaveTermination::UntilRarityAtLeast(Rarity::Uncommon));

        assert!(!wave_ends_after_nontoken(policy, Some(Rarity::Common), 0));
        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Uncommon), 0));
        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Rare), 0));
        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Mythic), 0));
    }

    /// The threshold is a parameter, not a constant — a deck may cap its waves
    /// only on rares.
    #[test]
    fn rarity_wave_threshold_is_parameterized() {
        let rare_only = Some(WaveTermination::UntilRarityAtLeast(Rarity::Rare));

        assert!(!wave_ends_after_nontoken(
            rare_only,
            Some(Rarity::Common),
            0
        ));
        assert!(!wave_ends_after_nontoken(
            rare_only,
            Some(Rarity::Uncommon),
            0
        ));
        assert!(wave_ends_after_nontoken(rare_only, Some(Rarity::Rare), 0));
        assert!(wave_ends_after_nontoken(rare_only, Some(Rarity::Mythic), 0));
    }

    /// A card-data gap must fail safe toward "keep revealing" rather than
    /// stalling the Horde on an unknown-rarity card.
    #[test]
    fn unknown_rarity_never_ends_a_rarity_wave() {
        let policy = Some(WaveTermination::UntilRarityAtLeast(Rarity::Uncommon));
        assert!(!wave_ends_after_nontoken(policy, None, 0));
    }

    /// `UntilNonToken` is rarity-blind — it ends on the nontoken QUOTA, not on
    /// what the card is. With the quota exhausted (0 remaining) any rarity ends
    /// the wave; this is the Cyberman `Fixed(1)` behavior, unchanged.
    #[test]
    fn until_nontoken_is_rarity_blind_once_the_quota_is_met() {
        let policy = Some(WaveTermination::UntilNonToken {
            count: WaveCount::Fixed(1),
        });

        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Common), 0));
        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Mythic), 0));
        assert!(wave_ends_after_nontoken(policy, None, 0));
    }

    /// A multi-nontoken wave keeps going while the quota is unmet, regardless of
    /// rarity — the Zombies Horde's Wave 2 and Wave 3 turns.
    #[test]
    fn until_nontoken_continues_while_quota_remains() {
        let policy = Some(WaveTermination::UntilNonToken {
            count: WaveCount::Snaking { min: 1, max: 3 },
        });

        assert!(!wave_ends_after_nontoken(policy, Some(Rarity::Mythic), 2));
        assert!(!wave_ends_after_nontoken(policy, Some(Rarity::Common), 1));
        assert!(wave_ends_after_nontoken(policy, Some(Rarity::Common), 0));
    }

    /// `FixedCount` waves are governed solely by the counter — casting a
    /// non-token never short-circuits them, at any rarity.
    #[test]
    fn fixed_count_and_absent_policy_never_end_the_wave_here() {
        let fixed = Some(WaveTermination::FixedCount(2));

        assert!(!wave_ends_after_nontoken(fixed, Some(Rarity::Common), 0));
        assert!(!wave_ends_after_nontoken(fixed, Some(Rarity::Mythic), 0));
        assert!(!wave_ends_after_nontoken(fixed, None, 0));
        assert!(!wave_ends_after_nontoken(None, Some(Rarity::Mythic), 0));
    }
}
