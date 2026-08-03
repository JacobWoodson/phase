use crate::types::format::FormatTopology;
use crate::types::format::GameFormat;
use crate::types::format::TurnStructure;
use crate::types::game_state::GameState;
use crate::types::player::PlayerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TeamId(pub u8);

/// One SIDE of the game: a group of seats that share a win/loss fate, together
/// with how that side takes turns. A side's `turn_structure` is its own axis, so
/// different sides can differ — the whole point of the general model: Horde's
/// survivor side takes a shared team turn (CR 805) while each Horde takes its own
/// individual turn (alternating, LOTR Two Towers), and Emperor's teams take
/// individual turns (CR 809.4). The other per-side axis — whether a side pools
/// life/poison (2HG's shared team vs Emperor's independent teammates, CR 809.7) —
/// is [`side_shares_life`], kept as a direct O(1) predicate (like [`team_id`])
/// rather than a field so it stays allocation-free in the hot SBA path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Side {
    pub seats: Vec<PlayerId>,
    pub turn_structure: TurnStructure,
}

/// The game's SIDES — the single grouping+turn-structure authority. [`team_members`]
/// derives its grouping from this; the turn-rotation / priority / APNAP functions
/// derive their per-side turn structure from it; [`team_id`] is kept as an O(1)
/// equality key and pinned consistent with this by `team_id_matches_side_index`.
///
/// Sides include EVERY seat (alive or not) so ordering is stable — a side's index
/// reproduces the historical [`TeamId`]: IndividualSeats is one side per seat
/// ordered by id; FixedTeams is one side per team; OneVsMany is the archenemy side
/// first, then the many (in seat order, matching the pre-refactor `team_members`).
/// Alive-filtering is the caller's job.
///
/// It currently REPRODUCES the three [`FormatTopology`] shapes exactly
/// (behavior-preserving): each side's turn structure is IndividualTurns for
/// IndividualSeats and the topology's own `turn_structure` for FixedTeams /
/// OneVsMany — so every side of a given format shares one structure today. Mixed
/// structures (Horde, Emperor) arrive when those formats emit their own sides.
pub(crate) fn sides(state: &GameState) -> Vec<Side> {
    // A two-Horde-force deck (LOTR Two Towers) emits a MIXED side shape that no
    // single `FormatTopology` variant can: the survivors form one side that takes
    // a shared team turn (CR 805), while both Horde seats form a single ALLIED
    // side (they never attack each other and share a collective loss) whose
    // members each take their own INDIVIDUAL, alternating turn. A single-Horde
    // game has no mixed structure — it is exactly the OneVsMany archenemy shape —
    // so it falls through to the topology match below.
    if let Some(hordes) = two_horde_seats(state) {
        // Seat order gives a stable side ordering (matching OneVsMany, which the
        // Horde replaces): survivors first (side 0), the Horde side second.
        let survivors: Vec<PlayerId> = state
            .seat_order
            .iter()
            .copied()
            .filter(|id| !hordes.contains(id))
            .collect();
        let horde_seats: Vec<PlayerId> = state
            .seat_order
            .iter()
            .copied()
            .filter(|id| hordes.contains(id))
            .collect();
        // Life-sharing is a per-side axis carried by [`side_shares_life`]: the
        // survivor side pools life (like 2HG), while the two allied Horde seats do
        // NOT pool with each other — each has no life total anyway.
        return vec![
            Side {
                seats: survivors,
                turn_structure: TurnStructure::SharedTeamTurns,
            },
            Side {
                seats: horde_seats,
                turn_structure: TurnStructure::IndividualTurns,
            },
        ];
    }

    match state.format_config.topology() {
        FormatTopology::IndividualSeats => {
            let mut ids: Vec<PlayerId> = state.players.iter().map(|p| p.id).collect();
            ids.sort_by_key(|id| id.0);
            ids.into_iter()
                .map(|id| Side {
                    seats: vec![id],
                    turn_structure: TurnStructure::IndividualTurns,
                })
                .collect()
        }
        FormatTopology::FixedTeams {
            team_size,
            team_count,
            turn_structure,
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
                .into_iter()
                .map(|seats| Side {
                    seats,
                    turn_structure,
                })
                .collect()
        }
        FormatTopology::OneVsMany {
            archenemy,
            turn_structure,
        } => {
            let many: Vec<PlayerId> = state
                .seat_order
                .iter()
                .copied()
                .filter(|&id| id != archenemy)
                .collect();
            vec![
                Side {
                    seats: vec![archenemy],
                    turn_structure,
                },
                Side {
                    seats: many,
                    turn_structure,
                },
            ]
        }
    }
}

/// The side that contains `player`, if any.
pub(crate) fn side_of(state: &GameState, player: PlayerId) -> Option<Side> {
    sides(state).into_iter().find(|s| s.seats.contains(&player))
}

/// The designated Horde seats when this game uses the two-Horde MIXED side shape
/// (a Horde game with 2+ designated seats), else `None`. A single-Horde game
/// (0 or 1 designated seats) is exactly the plain OneVsMany archenemy shape, so
/// this returns `None` and callers fall through to the [`FormatTopology`] paths.
///
/// Reads the `horde_seats` field directly (not [`crate::game::horde::horde_seats`],
/// which clones + falls back to the sole archenemy) so [`team_id`] stays an O(1),
/// allocation-free equality key. The `len() >= 2` gate is equivalent: the
/// fallback only ever yields 0 or 1 seat.
fn two_horde_seats(state: &GameState) -> Option<&[PlayerId]> {
    (state.format_config.format == GameFormat::Horde && state.horde_seats.len() >= 2)
        .then_some(state.horde_seats.as_slice())
}

pub(crate) fn team_id(state: &GameState, player: PlayerId) -> TeamId {
    // Two-Horde MIXED shape: survivors are side 0, the allied Horde seats side 1
    // (kept consistent with [`sides`] and pinned by `team_id_matches_side_index`).
    if let Some(hordes) = two_horde_seats(state) {
        return if hordes.contains(&player) {
            TeamId(1)
        } else {
            TeamId(0)
        };
    }
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
    side_of(state, player)
        .map(|side| side.seats)
        .unwrap_or_default()
        .into_iter()
        .filter(|&id| super::players::is_alive(state, id))
        .collect()
}

/// Whether `player`'s side takes a single shared team turn (CR 805) rather than
/// each member taking an individual turn. Per-side, so a mixed game (Horde:
/// survivors shared, Hordes individual) answers differently per player. This is
/// THE turn-structure predicate — it replaced a whole-format one, which could not
/// describe a mixed game — used both inside the turn-rotation / priority / APNAP
/// functions below AND at the peripheral consumers (land-drop, draw step,
/// turn-control authorization), each of which asks about the ACTIVE player's
/// side. For today's uniform formats every side of a game shares one structure,
/// so the answer matches the old whole-format predicate.
pub(crate) fn side_takes_shared_turn(state: &GameState, player: PlayerId) -> bool {
    side_of(state, player).is_some_and(|s| s.turn_structure == TurnStructure::SharedTeamTurns)
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

/// CR 810.4 / CR 810.8 / CR 810.9 / CR 810.10: Whether the game contains ANY side
/// that shares life, poison, and team loss (Two-Headed Giant, or a Horde game's
/// survivor side). Whole-game predicate used to pick the team-loss WIN algorithm
/// (`elimination`); the per-PLAYER question "does THIS player's side pool life"
/// is [`side_shares_life`], which is what all resource aggregation goes through.
///
/// Default Archenemy stays FALSE: it uses shared turns (CR 805) but its heroes do
/// NOT share life (CR 904.5, each hero at their own 20). Future Emperor teams also
/// stay FALSE (CR 809.7: teams do not share resources) — their format simply isn't
/// in this match.
pub(crate) fn has_shared_life_resources(state: &GameState) -> bool {
    matches!(
        state.format_config.format,
        GameFormat::TwoHeadedGiant | GameFormat::Horde
    )
}

/// CR 810.4 / CR 810.8 / CR 810.9 / CR 810.10: Whether `player`'s SIDE pools life,
/// poison, and life-lock statics as a team — the per-side life axis. This is the
/// single authority every per-player resource aggregation goes through
/// ([`shared_resource_members`]/[`shared_resource_dedup_key`], and the life-loss /
/// poison-loss / can't-gain-or-lose-life checks in `elimination`/`sba`/
/// `static_abilities`).
///
/// Computed directly (like [`team_id`]) rather than off [`sides`], so it stays an
/// O(1), allocation-free check in the hot SBA path:
/// - Two-Horde MIXED shape: the survivor side pools; each Horde seat does NOT (it
///   has no life total, and the two Horde seats are not a shared-life team). This
///   is the case a whole-game predicate gets wrong.
/// - Two-Headed Giant (`FixedTeams`): the team pools. A future individual-turn
///   `FixedTeams` (Emperor) does NOT (CR 809.7) — distinguished by format.
/// - Archenemy / single-Horde (`OneVsMany`): the "many" survivor side pools only
///   in Horde (CR 904.5 keeps Archenemy heroes independent); the archenemy/Horde
///   seat never pools.
/// - Free-for-all: never.
pub(crate) fn side_shares_life(state: &GameState, player: PlayerId) -> bool {
    if let Some(hordes) = two_horde_seats(state) {
        // Survivors pool; the allied (life-total-less) Horde seats do not.
        return !hordes.contains(&player);
    }
    match state.format_config.topology() {
        FormatTopology::IndividualSeats => false,
        FormatTopology::FixedTeams { .. } => {
            state.format_config.format == GameFormat::TwoHeadedGiant
        }
        FormatTopology::OneVsMany { archenemy, .. } => {
            player != archenemy && state.format_config.format == GameFormat::Horde
        }
    }
}

pub(crate) fn shared_resource_members(state: &GameState, player: PlayerId) -> Vec<PlayerId> {
    if side_shares_life(state, player) {
        team_members(state, player)
    } else if super::players::is_alive(state, player) {
        vec![player]
    } else {
        Vec::new()
    }
}

pub(crate) fn shared_resource_dedup_key(state: &GameState, player: PlayerId) -> TeamId {
    if side_shares_life(state, player) {
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

    // CR 101.4 + CR 103.1: APNAP follows the current turn-order direction. Per
    // side: a shared-team side chooses as ONE group (its living members), deduped
    // so it appears once; an individual-turn seat is its own group. For today's
    // uniform formats this reduces to the old two-branch behavior (all-individual
    // → one group per seat; all-shared → one group per team).
    let start_idx = seat_order
        .iter()
        .position(|&id| id == start_player)
        .unwrap_or(0);
    let mut seen_shared = std::collections::BTreeSet::new();
    let mut groups = Vec::new();
    for offset in 0..len {
        let idx = super::players::turn_order_index(start_idx, offset, len, state.turn_direction);
        let candidate = seat_order[idx];
        if !super::players::is_alive(state, candidate) {
            continue;
        }
        if side_takes_shared_turn(state, candidate) {
            // Dedup the shared side by its stable side key so it contributes one
            // group at its first-appearing member's position.
            if seen_shared.insert(team_dedup_key(state, candidate)) {
                groups.push(team_members(state, candidate));
            }
        } else {
            groups.push(vec![candidate]);
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
    if !side_takes_shared_turn(state, player) {
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
    // `normalize_shared_turn_recipient` already returns `player` unchanged when
    // their side takes individual turns, so this is correct per-side.
    normalize_shared_turn_recipient(state, player)
}

/// CR 805.4: In shared-team-turn formats, each team takes turns rather than
/// each player.
pub(crate) fn next_turn_representative(state: &GameState, current: PlayerId) -> PlayerId {
    // Individual-turn side: the next turn is simply the next living seat in turn
    // order (CR 103.1), normalized in case that seat belongs to a shared side — a
    // mixed game, e.g. the last individual Horde turn handing back to the shared
    // survivor side. For today's uniform individual formats the normalize is a
    // no-op, preserving the plain `next_player_in_turn_order` behavior.
    if !side_takes_shared_turn(state, current) {
        return normalize_shared_turn_recipient(
            state,
            super::players::next_player_in_turn_order(state, current),
        );
    }

    // Shared side (CR 805.4): the whole side took ONE turn, so skip the rest of it
    // and hand the next turn to the next living seat on a DIFFERENT side (its
    // representative).
    let seat_order = &state.seat_order;
    let len = seat_order.len();
    if len == 0 {
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
    // Map every APNAP player to its per-side priority representative (self for an
    // individual seat, the side's representative for a shared side) and dedup. For
    // uniform-individual formats every player is its own rep, so this returns the
    // plain APNAP order; for shared/mixed games it collapses each shared side to
    // one representative.
    super::players::apnap_order(state)
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

    /// `sides()` reproduces the historical grouping AND per-side turn structure
    /// for each `FormatTopology` shape — what makes the refactor behavior-preserving.
    #[test]
    fn sides_reproduce_the_topology_groupings() {
        let seats = |state: &GameState| -> Vec<Vec<PlayerId>> {
            sides(state).into_iter().map(|s| s.seats).collect()
        };
        let turn_kinds = |state: &GameState| -> Vec<TurnStructure> {
            sides(state).into_iter().map(|s| s.turn_structure).collect()
        };

        // Free-for-all: one side per seat, ordered by id, each individual-turn.
        let ffa = GameState::new(FormatConfig::free_for_all(), 4, 42);
        assert_eq!(
            seats(&ffa),
            vec![
                vec![PlayerId(0)],
                vec![PlayerId(1)],
                vec![PlayerId(2)],
                vec![PlayerId(3)],
            ]
        );
        assert!(turn_kinds(&ffa)
            .iter()
            .all(|t| *t == TurnStructure::IndividualTurns));

        // Two-Headed Giant: two teams of two (by id), each shared-turn.
        let thg = GameState::new(FormatConfig::two_headed_giant(), 4, 42);
        assert_eq!(
            seats(&thg),
            vec![
                vec![PlayerId(0), PlayerId(1)],
                vec![PlayerId(2), PlayerId(3)],
            ]
        );
        assert!(turn_kinds(&thg)
            .iter()
            .all(|t| *t == TurnStructure::SharedTeamTurns));

        // Archenemy: the archenemy side first, then the heroes; both shared-turn
        // (CR 904.2 uses the shared-team-turns option).
        let arch = GameState::new(FormatConfig::archenemy(), 4, 42);
        let s = sides(&arch);
        assert_eq!(s[0].seats, vec![PlayerId(0)], "archenemy side is first");
        let mut heroes = s[1].seats.clone();
        heroes.sort_by_key(|id| id.0);
        assert_eq!(heroes, vec![PlayerId(1), PlayerId(2), PlayerId(3)]);
        assert!(turn_kinds(&arch)
            .iter()
            .all(|t| *t == TurnStructure::SharedTeamTurns));
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
                    .position(|g| g.seats.contains(&p.id))
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

    /// A two-Horde-force game (LOTR Two Towers): seats 0 and 1 are the allied
    /// Horde seats, seats 2 and 3 the survivors. `seat_order` is pinned explicitly
    /// so the rotation assertions don't depend on construction details.
    fn two_horde_state() -> GameState {
        use crate::types::format::ChallengeDeck;
        let mut state = GameState::new(
            FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset()),
            4,
            42,
        );
        state.horde_seats = vec![PlayerId(0), PlayerId(1)];
        state.seat_order = vec![PlayerId(0), PlayerId(1), PlayerId(2), PlayerId(3)];
        state
    }

    /// The mixed side shape no `FormatTopology` variant can express: the survivors
    /// are one shared-turn side, both Horde seats a single individual-turn side.
    #[test]
    fn two_horde_game_splits_survivors_and_allied_hordes_into_sides() {
        let state = two_horde_state();
        let s = sides(&state);
        assert_eq!(s.len(), 2, "survivors + Horde side");

        assert_eq!(
            s[0].seats,
            vec![PlayerId(2), PlayerId(3)],
            "survivors first"
        );
        assert_eq!(s[0].turn_structure, TurnStructure::SharedTeamTurns);

        assert_eq!(
            s[1].seats,
            vec![PlayerId(0), PlayerId(1)],
            "both Horde seats form one side"
        );
        assert_eq!(
            s[1].turn_structure,
            TurnStructure::IndividualTurns,
            "each Horde takes its own turn"
        );

        // The O(1) `team_id` key cannot diverge from the mixed `sides()` grouping.
        for p in &state.players {
            let idx = s.iter().position(|g| g.seats.contains(&p.id)).unwrap();
            assert_eq!(team_id(&state, p.id), TeamId(idx as u8));
        }
    }

    /// The two Horde seats are allies (they never attack each other and share a
    /// collective loss); each opposes every survivor.
    #[test]
    fn two_horde_seats_are_allies_survivors_are_their_opponents() {
        let state = two_horde_state();
        assert!(
            !is_opponent(&state, PlayerId(0), PlayerId(1)),
            "the two Hordes are allied"
        );
        assert!(
            !is_opponent(&state, PlayerId(2), PlayerId(3)),
            "the survivors are allied"
        );
        assert!(is_opponent(&state, PlayerId(0), PlayerId(2)));
        assert!(is_opponent(&state, PlayerId(1), PlayerId(3)));
        assert_eq!(
            teammates(&state, PlayerId(0)),
            vec![PlayerId(1)],
            "a Horde seat's only ally is the other Horde seat"
        );
    }

    /// Turn rotation treats each Horde seat individually but the survivors as one
    /// shared turn: H0 → H1 → (one) survivor turn → back to H0.
    #[test]
    fn two_horde_rotation_gives_each_horde_a_turn_then_one_shared_survivor_turn() {
        let state = two_horde_state();
        // Each Horde seat takes its own individual turn.
        assert_eq!(next_turn_representative(&state, PlayerId(0)), PlayerId(1));
        // After the last Horde, the survivors take ONE shared turn (its rep).
        assert_eq!(next_turn_representative(&state, PlayerId(1)), PlayerId(2));
        // That single shared survivor turn hands straight back to the first Horde —
        // the other survivor does NOT get a separate turn.
        assert_eq!(next_turn_representative(&state, PlayerId(2)), PlayerId(0));
    }

    /// Priority: each individual Horde seat participates on its own, while the
    /// survivor side collapses to a single representative.
    #[test]
    fn two_horde_priority_participants_collapse_survivors_but_not_hordes() {
        let mut state = two_horde_state();
        state.active_player = PlayerId(0);
        assert_eq!(
            priority_pass_participants(&state),
            vec![PlayerId(0), PlayerId(1), PlayerId(2)]
        );
    }

    /// `side_shares_life` is per-side: the survivor side pools (2HG-style) while
    /// each Horde seat does not — the two allied Horde seats must NOT share a life
    /// pool with each other (the vestigial-pool bug a whole-game predicate had).
    #[test]
    fn two_horde_survivors_pool_life_hordes_do_not() {
        let state = two_horde_state();
        assert!(side_shares_life(&state, PlayerId(2)), "survivor pools");
        assert!(side_shares_life(&state, PlayerId(3)), "survivor pools");
        assert!(
            !side_shares_life(&state, PlayerId(0)),
            "Horde seat does not"
        );
        assert!(
            !side_shares_life(&state, PlayerId(1)),
            "Horde seat does not"
        );

        // The resource-aggregation members follow suit: a Horde seat aggregates
        // over itself alone (not both Hordes), a survivor over the survivor team.
        assert_eq!(
            shared_resource_members(&state, PlayerId(0)),
            vec![PlayerId(0)]
        );
        let mut survivor_pool = shared_resource_members(&state, PlayerId(2));
        survivor_pool.sort_by_key(|id| id.0);
        assert_eq!(survivor_pool, vec![PlayerId(2), PlayerId(3)]);
    }

    /// `side_shares_life` across the uniform formats: 2HG pools, Archenemy heroes
    /// stay independent (CR 904.5), free-for-all never pools.
    #[test]
    fn side_shares_life_matches_each_uniform_format() {
        let thg = GameState::new(FormatConfig::two_headed_giant(), 4, 42);
        assert!(thg.players.iter().all(|p| side_shares_life(&thg, p.id)));

        let arch = GameState::new(FormatConfig::archenemy(), 4, 42);
        assert!(
            arch.players.iter().all(|p| !side_shares_life(&arch, p.id)),
            "Archenemy heroes each keep their own 20 (CR 904.5)"
        );

        let ffa = GameState::new(FormatConfig::free_for_all(), 4, 42);
        assert!(ffa.players.iter().all(|p| !side_shares_life(&ffa, p.id)));
    }
}
