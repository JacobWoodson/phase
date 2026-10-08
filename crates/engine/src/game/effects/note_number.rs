use crate::game::quantity::resolve_quantity_with_targets;
use crate::types::ability::{Effect, EffectError, ResolvedAbility};
use crate::types::events::GameEvent;
use crate::types::game_state::GameState;

/// Digital-only Alchemy (no CR entry): `Effect::NoteNumber` — evaluate
/// `value` and record it as the resolving player's noted number
/// (`Player::noted_number`), overwriting any previous note ("note its
/// power", Dragonborn Immolator; "note that excess damage", Mephit's
/// Enthusiasm / Molten Impact). Read back by `QuantityRef::NotedNumber`
/// ("where X is the noted number") when the granted one-time boon later
/// triggers.
///
/// The noting player is `original_controller.unwrap_or(controller)` — the
/// same subject `resolve_quantity_with_targets` resolves `value` under —
/// so a note and its read agree even under `player_scope` fanout.
///
/// Doing the write at resolution — not when the trigger fires — means a
/// countered or otherwise removed-from-stack ability never notes anything
/// (CR 608.2c: instructions are followed only on resolution), mirroring
/// `note_mana_spent`.
pub fn resolve(
    state: &mut GameState,
    ability: &ResolvedAbility,
    _events: &mut Vec<GameEvent>,
) -> Result<(), EffectError> {
    let Effect::NoteNumber { value } = &ability.effect else {
        return Err(EffectError::MissingParam("NoteNumber".to_string()));
    };

    let noted = resolve_quantity_with_targets(state, value, ability);
    let noting_player = ability.original_controller.unwrap_or(ability.controller);
    if let Some(player) = state.players.iter_mut().find(|p| p.id == noting_player) {
        player.noted_number = Some(noted);
    }

    Ok(())
}
