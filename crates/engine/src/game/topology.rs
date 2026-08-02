use crate::types::format::FormatTopology;
use crate::types::format::GameFormat;
use crate::types::game_state::GameState;
use crate::types::player::PlayerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TeamId(pub u8);

/// The game's SIDES — the groups of seats that share a win/loss fate (and, per
/// format, may share turns or life). This is the single grouping authority that
/// [`team_members`] derives from; [`team_id`] is kept as an O(1) equality key and
/// pinned consistent with this by `team_id_matches_side_index` in tests.
///
/// Groups include EVERY seat (alive or not) so the ordering is stable — a side's
/// index reproduces the historical [`TeamId`]: IndividualSeats is one side per
/// seat ordered by id; FixedTeams is one side per team ordered by team index;
/// OneVsMany is the archenemy side first, then the many (in seat order, matching
/// the pre-refactor `team_members`). Alive-filtering is the caller's job.
///
/// Stage 1 of the general sides-based topology: it currently REPRODUCES the three
/// [`FormatTopology`] shapes exactly (behavior-preserving). Later stages give each
/// side its own turn structure + resource policy so mixed models become
/// expressible — Horde's survivors-share-a-turn + individual alternating Horde
/// turns, and Emperor's individual-turn teams (CR 809.4).
pub(crate) fn sides(state: &GameState) -> Vec<Vec<PlayerId>> {
    match state.format_config.topology() {
        FormatTopology::IndividualSeats => {
            let mut ids: Vec<PlayerId> = state.players.iter().map(|p| p.id).collect();
            ids.sort_by_key(|id| id.0);
            ids.into_iter().map(|id| vec![id]).collect()
        }
        FormatTopology::FixedTeams {
            team_size,
            team_count,
            ..
        } => {
            let mut teams: Vec<Vec<PlayerId>> = vec![Vec::new(); team_count as usize];
            let mut ids: Vec<PlayerId> = state.players.iter().map(|p| p.id).collect();
            ids.sort_by_key(|id| id.0);
            for id in ids {
                let team = (id.0 / team_size) as usize;
                if let Some(members) = teams.get_mut(team) {
                    members.push(id);
                }
            }
            teams
        }
        FormatTopology::OneVsMany { archenemy, .. } => {
            let many: Vec<PlayerId> = state
                .seat_order
                .iter()
                .copied()
                .filter(|&id| id != archenemy)
                .collect();
            vec![vec![archenemy], many]
        }
    }
}

pub(crate) fn team_id(state: &GameState, player: PlayerId) -> TeamId {
    match state.format_config.topology() {
        FormatTopology::IndividualSeats => TeamId(player.0),
        FormatTopology::FixedTeams { team_size, .. } => TeamId(player.0 / team_size),
        FormatTopology::OneVsMany { archenemy, .. } => {
            if player == archenemy {
                TeamId(0)
            } else {
                TeamId(1)
            }
        }
    }
}

pub(crate) fn team_members(state: &GameState, player: PlayerId) -> Vec<PlayerId> {
    // The living members of `player`'s side (see [`sides`], the grouping
    // authority). A player with no side (shouldn't happen) has no teammates.
    sides(state)
        .into_iter()
        .find(|side| side.contains(&player))
        .unwrap_or_default()
        .into_iter()
        .filter(|&id| super::players::is_alive(state, id))
        .collect()
}

pub(crate) fn teammates(state: &GameState, player: PlayerId) -> Vec<PlayerId> {
    match state.format_config.topology() {
        FormatTopology::IndividualSeats => Vec::new(),
        FormatTopology::FixedTeams { .. } | FormatTopology::OneVsMany { .. } => {
            team_members(state, player)
                .into_iter()
                .filter(|&id| id != player)
                .collect()
        }
    }
}

pub(crate) fn is_opponent(state: &GameState, player: PlayerId, other: PlayerId) -> bool {
    player != other && team_id(state, player) != team_id(state, other)
}

pub(crate) fn team_dedup_key(state: &GameState, player: PlayerId) -> TeamId {
    team_id(state, player)
}

pub(crate) fn archenemy(state: &GameState) -> Option<PlayerId> {
    match state.format_config.topology() {
        FormatTopology::OneVsMany { archenemy, .. } => Some(archenemy),
        FormatTopology::IndividualSeats | FormatTopology::FixedTeams { .. } => None,
    }
}

/// CR 810.4 / CR 810.8 / CR 810.9 / CR 810.10: Formats whose team shares life,
/// poison, and team loss. Two-Headed Giant is the canonical case; Horde Magic
/// reuses the same shared-resource rules for its survivor team (2–4 survivors
/// share one combined life total, Theros/Cyberman style). The survivors also
/// share POISON and the dedup grouping via this predicate — intended and
/// harmless: the Horde deals damage, never poison, so no Horde source ever adds
/// a poison counter to a survivor.
///
/// Default Archenemy stays FALSE: it uses shared turns (CR 805) but its heroes
/// do NOT share life (CR 904.5, each hero at their own 20). The explicit
/// `TwoHeadedGiant | Horde` match (not a topology check) preserves that — both
/// Horde and Archenemy map to `OneVsMany`, so only the format enum distinguishes
/// them.
pub(crate) fn has_shared_life_resources(state: &GameState) -> bool {
    matches!(
        state.format_config.format,
        GameFormat::TwoHeadedGiant | GameFormat::Horde
    )
}

pub(crate) fn shared_resource_members(state: &GameState, player: PlayerId) -> Vec<PlayerId> {
    if has_shared_life_resources(state) {
        team_members(state, player)
    } else if super::players::is_alive(state, player) {
        vec![player]
    } else {
        Vec::new()
    }
}

pub(crate) fn shared_resource_dedup_key(state: &GameState, player: PlayerId) -> TeamId {
    if has_shared_life_resources(state) {
        team_id(state, player)
    } else {
        TeamId(player.0)
    }
}

pub(crate) fn apnap_choice_groups(state: &GameState) -> Vec<Vec<PlayerId>> {
    apnap_choice_groups_from(state, state.active_player)
}

pub(crate) fn apnap_choice_groups_from(
    state: &GameState,
    start_player: PlayerId,
) -> Vec<Vec<PlayerId>> {
    let seat_order = &state.seat_order;
    let len = seat_order.len();
    if len == 0 {
        return Vec::new();
    }

    if !state.format_config.topology().has_shared_team_turns() {
        let start_idx = seat_order
            .iter()
            .position(|&id| id == start_player)
            .unwrap_or(0);
        return (0..len)
            .filter_map(|offset| {
                // CR 101.4 + CR 103.1: APNAP follows the current turn-order direction.
                let idx =
                    super::players::turn_order_index(start_idx, offset, len, state.turn_direction);
                let candidate = seat_order[idx];
                super::players::is_alive(state, candidate).then_some(vec![candidate])
            })
            .collect();
    }

    let start_idx = seat_order
        .iter()
        .position(|&id| id == start_player)
        .unwrap_or(0);
    let mut seen = std::collections::BTreeSet::new();
    let mut groups = Vec::new();
    for offset in 0..len {
        // CR 101.4 + CR 103.1: APNAP follows the current turn-order direction.
        let idx = super::players::turn_order_index(start_idx, offset, len, state.turn_direction);
        let candidate = seat_order[idx];
        if !super::players::is_alive(state, candidate) {
            continue;
        }
        let key = team_dedup_key(state, candidate);
        if seen.insert(key) {
            groups.push(team_members(state, candidate));
        }
    }
    groups
}

pub(crate) fn apnap_order_from(state: &GameState, start_player: PlayerId) -> Vec<PlayerId> {
    apnap_choice_groups_from(state, start_player)
        .into_iter()
        .flatten()
        .collect()
}

pub(crate) fn apnap_team_rank(state: &GameState, player: PlayerId) -> usize {
    let groups = apnap_choice_groups(state);
    groups
        .iter()
        .position(|group| group.contains(&player))
        .unwrap_or(groups.len())
}

pub(crate) fn normalize_shared_turn_recipient(state: &GameState, player: PlayerId) -> PlayerId {
    if !state.format_config.topology().has_shared_team_turns() {
        return player;
    }

    team_members(state, player)
        .into_iter()
        .next()
        .unwrap_or(player)
}

/// CR 117.6 + CR 805.5b: In shared-team-turn multiplayer games, teams rather
/// than individual players have priority; when no player on a team acts, that
/// team passes.
pub(crate) fn priority_pass_representative(state: &GameState, player: PlayerId) -> PlayerId {
    if !state.format_config.topology().has_shared_team_turns() {
        return player;
    }

    normalize_shared_turn_recipient(state, player)
}

/// CR 805.4: In shared-team-turn formats, each team takes turns rather than
/// each player.
pub(crate) fn next_turn_representative(state: &GameState, current: PlayerId) -> PlayerId {
    if !state.format_config.topology().has_shared_team_turns() {
        // CR 103.1: the next turn proceeds in the current turn-order direction.
        return super::players::next_player_in_turn_order(state, current);
    }

    let seat_order = &state.seat_order;
    let len = seat_order.len();
    if seat_order.is_empty() {
        return normalize_shared_turn_recipient(state, current);
    }

    let current_team = team_id(state, current);
    let current_idx = seat_order.iter().position(|&id| id == current).unwrap_or(0);

    for offset in 1..=len {
        // CR 103.1: walk seats in the current turn-order direction.
        let idx = super::players::turn_order_index(current_idx, offset, len, state.turn_direction);
        let candidate = seat_order[idx];
        if super::players::is_alive(state, candidate) && team_id(state, candidate) != current_team {
            return normalize_shared_turn_recipient(state, candidate);
        }
    }

    normalize_shared_turn_recipient(state, current)
}

pub(crate) fn priority_pass_participants(state: &GameState) -> Vec<PlayerId> {
    let participants = super::players::apnap_order(state);
    if !state.format_config.topology().has_shared_team_turns() {
        return participants;
    }

    participants
        .into_iter()
        .map(|player| priority_pass_representative(state, player))
        .fold(Vec::new(), |mut reps, rep| {
            if !reps.contains(&rep) {
                reps.push(rep);
            }
            reps
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::format::FormatConfig;

    #[test]
    fn next_turn_representative_reverses_with_turn_direction() {
        use crate::types::phase::TurnDirection;
        let mut state = GameState::new(FormatConfig::free_for_all(), 4, 42);
        // CR 103.1: normal turn order walks forward (P0 → P1).
        assert_eq!(next_turn_representative(&state, PlayerId(0)), PlayerId(1));
        state.turn_direction = TurnDirection::Reversed;
        // Reversed: the next turn walks backward (P0 → P3).
        assert_eq!(next_turn_representative(&state, PlayerId(0)), PlayerId(3));
    }

    #[test]
    fn two_hg_priority_pass_participants_are_team_representatives() {
        let mut state = GameState::new(FormatConfig::two_headed_giant(), 4, 42);
        state.active_player = PlayerId(0);

        assert_eq!(
            priority_pass_participants(&state),
            vec![PlayerId(0), PlayerId(2)]
        );
    }

    #[test]
    fn two_hg_priority_pass_representative_uses_living_teammate() {
        let mut state = GameState::new(FormatConfig::two_headed_giant(), 4, 42);
        state.active_player = PlayerId(0);
        state.players[0].is_eliminated = true;
        state.eliminated_players.push(PlayerId(0));

        assert_eq!(
            priority_pass_representative(&state, PlayerId(0)),
            PlayerId(1)
        );
        assert_eq!(
            priority_pass_participants(&state),
            vec![PlayerId(1), PlayerId(2)]
        );
    }

    #[test]
    fn archenemy_team_members_by_side_for_supported_player_counts() {
        for player_count in [2, 4, 6] {
            let state = GameState::new(FormatConfig::archenemy(), player_count, 42);

            assert_eq!(archenemy(&state), Some(PlayerId(0)));
            assert_eq!(team_members(&state, PlayerId(0)), vec![PlayerId(0)]);

            let heroes: Vec<PlayerId> = (1..player_count).map(PlayerId).collect();
            assert_eq!(team_members(&state, PlayerId(1)), heroes);
        }
    }

    #[test]
    fn archenemy_team_members_exclude_eliminated_heroes() {
        let mut state = GameState::new(FormatConfig::archenemy(), 6, 42);
        state.players[2].is_eliminated = true;
        state.eliminated_players.push(PlayerId(2));

        assert_eq!(
            team_members(&state, PlayerId(1)),
            vec![PlayerId(1), PlayerId(3), PlayerId(4), PlayerId(5)]
        );
        assert_eq!(
            teammates(&state, PlayerId(1)),
            vec![PlayerId(3), PlayerId(4), PlayerId(5)]
        );
    }

    /// `sides()` reproduces the historical grouping for each `FormatTopology`
    /// shape — this is what makes the Stage 1 refactor behavior-preserving.
    #[test]
    fn sides_reproduce_the_topology_groupings() {
        // Free-for-all: one side per seat, ordered by id.
        let ffa = GameState::new(FormatConfig::free_for_all(), 4, 42);
        assert_eq!(
            sides(&ffa),
            vec![
                vec![PlayerId(0)],
                vec![PlayerId(1)],
                vec![PlayerId(2)],
                vec![PlayerId(3)],
            ]
        );

        // Two-Headed Giant: two teams of two, by id.
        let thg = GameState::new(FormatConfig::two_headed_giant(), 4, 42);
        assert_eq!(
            sides(&thg),
            vec![
                vec![PlayerId(0), PlayerId(1)],
                vec![PlayerId(2), PlayerId(3)],
            ]
        );

        // Archenemy: the archenemy side first, then the heroes.
        let arch = GameState::new(FormatConfig::archenemy(), 4, 42);
        let s = sides(&arch);
        assert_eq!(s[0], vec![PlayerId(0)], "archenemy side is first");
        let mut heroes = s[1].clone();
        heroes.sort_by_key(|id| id.0);
        assert_eq!(heroes, vec![PlayerId(1), PlayerId(2), PlayerId(3)]);
    }

    /// The O(1) `team_id` equality key must never diverge from the full `sides()`
    /// grouping: `team_id(player)` equals the index of the player's side.
    #[test]
    fn team_id_matches_side_index() {
        for state in [
            GameState::new(FormatConfig::free_for_all(), 4, 42),
            GameState::new(FormatConfig::two_headed_giant(), 4, 42),
            GameState::new(FormatConfig::archenemy(), 6, 42),
        ] {
            let groups = sides(&state);
            for p in &state.players {
                let idx = groups
                    .iter()
                    .position(|g| g.contains(&p.id))
                    .expect("every seat belongs to a side");
                assert_eq!(
                    team_id(&state, p.id),
                    TeamId(idx as u8),
                    "team_id must equal the sides() index for {:?} in {:?}",
                    p.id,
                    state.format_config.format
                );
            }
        }
    }
}
