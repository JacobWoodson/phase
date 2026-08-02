import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TargetRef, WaitingFor } from "../../../adapter/types.ts";
import { useGameStore } from "../../../stores/gameStore.ts";
import { useMultiplayerStore } from "../../../stores/multiplayerStore.ts";
import {
  buildFormatConfig,
  buildGameState,
  buildPendingCast,
  buildPlayers,
  buildTargetSelectionProgress,
  buildTargetSelectionSlot,
  buildTargetSelectionWaitingFor,
} from "../../../test/factories/gameStateFactory.ts";
import { OpponentSeatHeader } from "../OpponentSeatHeader.tsx";

function targetSelectionWaitingFor(legalPlayers: number[]): WaitingFor {
  const targets: TargetRef[] = legalPlayers.map((player) => ({ Player: player }));
  return buildTargetSelectionWaitingFor({
    data: {
      player: 0,
      selection: buildTargetSelectionProgress({ current_legal_targets: targets }),
      target_slots: [buildTargetSelectionSlot({ legal_targets: targets })],
      pending_cast: buildPendingCast(),
    },
  });
}

function createGameState(waitingFor: WaitingFor) {
  return buildGameState({
    players: buildPlayers([
      { id: 0, life: 40 },
      { id: 1, life: 40 },
      { id: 2, life: 40 },
      { id: 3, life: 40 },
    ]),
    waiting_for: waitingFor,
    seat_order: [0, 1, 2, 3],
    eliminated_players: [],
  });
}

// A Horde game: seat 1 is the Horde (engine flag `has_no_life_total`), and the
// format config carries the challenge deck the rules badge looks up.
function createHordeGameState(waitingFor: WaitingFor) {
  return buildGameState({
    players: buildPlayers([
      { id: 0, life: 100 },
      { id: 1, life: 0, has_no_life_total: true },
    ]),
    format_config: buildFormatConfig({
      format: "Horde",
      horde_ruleset: {
        challenge_deck: "CybermanHorde",
        wave: { type: "UntilNonToken", data: { count: { type: "Fixed", data: 1 } } },
        survivor_setup_turns: 3,
        combined_base_life: 100,
        per_extra_survivor_life_delta: -15,
        horde_creatures_forced_attackers: true,
      },
    }),
    waiting_for: waitingFor,
    seat_order: [0, 1],
    eliminated_players: [],
  });
}

describe("OpponentSeatHeader", () => {
  beforeEach(() => {
    useMultiplayerStore.setState({ activePlayerId: 0 });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("targets the opponent when the whole legal target plate is clicked", () => {
    const dispatch = vi.fn();
    const waitingFor = targetSelectionWaitingFor([1]);
    useGameStore.setState({
      dispatch,
      gameState: createGameState(waitingFor),
      waitingFor,
    });

    render(<OpponentSeatHeader playerId={1} />);

    fireEvent.click(screen.getByRole("button", { name: "Target Opp 2" }));

    expect(dispatch).toHaveBeenCalledWith({
      type: "ChooseTarget",
      data: { target: { Player: 1 } },
    });
  });

  it("does not target when the opponent player is not legal", () => {
    const dispatch = vi.fn();
    const waitingFor = targetSelectionWaitingFor([2]);
    useGameStore.setState({
      dispatch,
      gameState: createGameState(waitingFor),
      waitingFor,
    });

    render(<OpponentSeatHeader playerId={1} />);

    fireEvent.click(screen.getByTestId("opponent-seat-header-1"));

    expect(screen.queryByRole("button", { name: "Target Opp 2" })).not.toBeInTheDocument();
    expect(dispatch).not.toHaveBeenCalled();
  });

  it("surfaces player-attached Auras (curses) for parity with the legacy HUD", () => {
    const waitingFor = targetSelectionWaitingFor([]);
    useGameStore.setState({
      gameState: {
        ...createGameState(waitingFor),
        derived: {
          auras_attached_to_player: { "1": [101, 102] },
        },
      },
      waitingFor,
    });

    render(<OpponentSeatHeader playerId={1} />);

    expect(
      screen.getByRole("button", { name: "2 enchantments on this player" }),
    ).toBeInTheDocument();
  });

  it("renders Next Up badge with tooltip text", () => {
    const waitingFor = targetSelectionWaitingFor([]);
    useGameStore.setState({
      gameState: {
        ...createGameState(waitingFor),
        derived: {
          turn_order: [{ player: 1, slot_index: 1, turns_from_now: 1 }],
        },
      },
      waitingFor,
    });

    render(<OpponentSeatHeader playerId={1} />);

    expect(screen.getByTitle("This player's turn is next.")).toHaveTextContent("Next Up");
  });

  it("shows the Horde rules badge on the Horde seat and lists the deck's engine rules", () => {
    const waitingFor = targetSelectionWaitingFor([]);
    useGameStore.setState({ gameState: createHordeGameState(waitingFor), waitingFor });

    render(<OpponentSeatHeader playerId={1} />);

    const badge = screen.getByRole("img", { name: "Horde deck rules" });
    expect(badge).toBeInTheDocument();

    // Hovering surfaces the engine-authored rules for the configured deck,
    // resolved through the challenge-deck registry (no client-side derivation).
    fireEvent.mouseEnter(badge);
    expect(screen.getByText("Cyberman Horde")).toBeInTheDocument();
    expect(
      screen.getByText("Reveals until the first nontoken card is cast"),
    ).toBeInTheDocument();
  });

  it("does not show the Horde rules badge on a normal (non-Horde) opponent seat", () => {
    const waitingFor = targetSelectionWaitingFor([]);
    useGameStore.setState({ gameState: createGameState(waitingFor), waitingFor });

    render(<OpponentSeatHeader playerId={1} />);

    expect(
      screen.queryByRole("img", { name: "Horde deck rules" }),
    ).not.toBeInTheDocument();
  });
});
