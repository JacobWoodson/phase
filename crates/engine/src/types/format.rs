use serde::{Deserialize, Serialize};

use crate::database::legality::LegalityFormat;
use crate::types::card::Rarity;
use crate::types::player::PlayerId;

/// Broad grouping used by the UI to visually cluster related formats
/// (constructed, commander-style, multiplayer). Frontends may key color
/// treatments off the group so they don't have to maintain a per-format
/// styling table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormatGroup {
    Constructed,
    Commander,
    Multiplayer,
    Limited,
}

/// Authoritative metadata for a single user-selectable format. Produced by
/// `GameFormat::registry()` and consumed by the frontend so that adding a new
/// format requires touching the engine only — no mirrored maps on the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatMetadata {
    pub format: GameFormat,
    /// Full display label, e.g. "Historic Brawl".
    pub label: &'static str,
    /// Short three-letter code for compact badges, e.g. "HBR".
    pub short_label: &'static str,
    /// One-line human description suitable for a card or tooltip.
    pub description: &'static str,
    pub group: FormatGroup,
    pub default_config: FormatConfig,
}

/// Supported game formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameFormat {
    Standard,
    Limited,
    Commander,
    Pioneer,
    Modern,
    Premodern,
    Legacy,
    Vintage,
    Historic,
    Timeless,
    Pauper,
    PauperCommander,
    DuelCommander,
    TinyLeaders,
    Oathbreaker,
    Brawl,
    HistoricBrawl,
    FreeForAll,
    TwoHeadedGiant,
    /// CR 904: Default Archenemy — one archenemy faces a team of heroes using
    /// shared team turns (CR 805), with a single scheme deck (CR 904.3).
    Archenemy,
    /// CR 901: Planechase using the single communal planar deck option
    /// (CR 901.15a), plus normal 60-card player decks.
    Planechase,
    /// Momir's Madness: 60 snow basic lands (12 each, no Snow-Covered Wastes),
    /// 20 life, a game-start command-zone emblem granting "{X}, Discard a card:
    /// Create a token that's a copy of a creature card with mana value X chosen
    /// at random."
    Momir,
    /// Horde Magic: a cooperative variant where a team of survivors faces a
    /// single self-piloting "Horde" deck. Casual community format (not
    /// DCI/CR-sanctioned); it reuses CR mechanisms the engine already models —
    /// shared team turns (CR 805) for the survivor team and the one-vs-many
    /// topology used by Archenemy (CR 904). The Horde has no life total, takes
    /// scripted "reveal and resolve" turns, and loses by decking out. Which
    /// concrete deck is played and how each Horde turn reveals cards are carried
    /// by `FormatConfig::horde_ruleset` (`HordeRuleset`), never by sibling
    /// `GameFormat` variants.
    Horde,
}

/// CR 100.4 / CR 100.4a: Per-format sideboard rules.
///
/// - `Forbidden`: the format does not have a sideboard at all (Commander, Brawl,
///   Historic Brawl). Semantically distinct from `Limited(0)` — those formats
///   don't "have" a zero-size sideboard, they have no sideboard concept.
/// - `Limited(n)`: constructed formats cap the sideboard at `n` cards.
///   CR 100.4a sets this at 15 for standard constructed play.
/// - `Unlimited`: casual multiplayer variants (Free-for-All, Two-Headed Giant)
///   impose no size constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SideboardPolicy {
    Forbidden,
    Limited(u32),
    Unlimited,
}

/// Per-card override to the default constructed copy limit.
///
/// CR 100.2a sets the default constructed limit to four of any card with a
/// particular English name (basic lands excepted). A handful of cards print an
/// explicit deck-construction override in their rules text:
///
/// - `Unlimited`: "A deck can have any number of cards named ~." (Relentless
///   Rats, Shadowborn Apostle, etc.) — no upper bound on copies.
/// - `UpTo(n)`: "A deck can have up to <n> cards named ~." (Seven Dwarves → 7,
///   Nazgûl → 9) and the Commander/companion singleton override "Your deck can
///   have only one copy of this card" (Vazal, the Compleat → `UpTo(1)`).
///
/// CR 903.5b's Commander singleton rule exempts basic lands; an `UpTo(n>1)`
/// override likewise raises the cap above the format default for that card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DeckCopyLimit {
    Unlimited,
    UpTo(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStructure {
    IndividualTurns,
    SharedTeamTurns,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatTopology {
    IndividualSeats,
    FixedTeams {
        team_size: u8,
        team_count: u8,
        turn_structure: TurnStructure,
    },
    OneVsMany {
        archenemy: PlayerId,
        turn_structure: TurnStructure,
    },
}

/// Which concrete self-piloting Horde deck a `GameFormat::Horde` game uses.
///
/// This is the deck-identity axis of the Horde ruleset. Per-deck rule
/// differences (wave size, whether the Horde's creatures attack, life totals)
/// live on `HordeRuleset`, not here — this enum only names the deck so
/// `deck_loading` can inject the right fixed library. New decks are pure
/// additions: the community/Knudson variants and the three Theros challenge
/// decks (Battle the Horde, Face the Hydra, Defeat a God) each add one variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeDeck {
    /// Doctor Who "Cyberman Horde" — ~100 real Universes Beyond cards plus ~200
    /// predefined Cyberman/Dalek tokens. The first deck the spine is validated
    /// against.
    CybermanHorde,
    /// D&D Horde (community format, hordemagic.com). An escalating challenge of
    /// three tiered libraries (Ooze → Goblin/Skeleton → Giant/Dragon), all built
    /// from real, already-implemented cards, using the rarity-based wave rule
    /// (`WaveTermination::UntilRarityAtLeast`). This spine slice loads the Level 1
    /// **Ooze** library; multi-tier progression is a follow-up.
    DndHorde,
    /// Zombies Horde (community format, hordemagic.com). 100 nontoken cards plus
    /// 200 Zombie / Zombie Giant tokens. Its signature rule is a *snaking* wave
    /// count — the Horde reveals until one nontoken resolves, then two, then
    /// three, then back down — so pressure ramps up and eases off in a cycle.
    ZombiesHorde,
    /// Slivers Horde (community format, hordemagic.com). 135 nontoken cards plus
    /// 170 Metallic Sliver tokens. Every Sliver lord buffs every other Sliver, so
    /// the board compounds fast — the swarm gets stronger rather than just wider.
    SliversHorde,
    /// Humans and Godzilla Horde (community format, hordemagic.com). 100 nontoken
    /// cards plus 200 Human Soldier tokens: a wide, cheap human army punctuated
    /// by a handful of enormous Godzilla-series titans.
    HumansGodzillaHorde,
}

/// Authoritative display metadata for one selectable Horde challenge deck.
///
/// Produced by [`ChallengeDeck::registry`] and consumed by the frontend's Horde
/// deck picker, so adding a new challenge deck requires touching the engine only
/// — the client never maintains a mirrored deck list. Mirrors [`FormatMetadata`]
/// for `GameFormat`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeDeckMetadata {
    pub deck: ChallengeDeck,
    /// Full display label, e.g. "Cyberman Horde".
    pub label: &'static str,
    /// Short code for compact badges, e.g. "CYB".
    pub short_label: &'static str,
    /// One-line human description suitable for a card or tooltip.
    pub description: &'static str,
    /// The deck's canonical ruleset, so a client can build the complete
    /// `FormatConfig` for a selected deck without a second call.
    pub default_ruleset: HordeRuleset,
    /// Human-readable, engine-authored breakdown of how this deck plays — one
    /// labeled line per rules axis (waves, survivor life, setup, Horde combat).
    /// Rendered by [`HordeRuleset::summary`] from the structured ruleset, so the
    /// frontend displays these verbatim and never derives rules prose from the
    /// typed axes itself (that would put game-rule interpretation in the display
    /// layer). Lets the deck picker and any in-game info panel show HOW each deck
    /// plays and how it DIFFERS without a second source of truth.
    pub rules: Vec<RuleSummaryLine>,
}

/// One labeled line of a Horde deck's human-readable rules summary.
///
/// The `label` is a short, stable category name the frontend can style (bold,
/// column header, etc.); the `detail` is the deck-specific behavior for that
/// category, already rendered into prose by the engine. Both are display text —
/// the source of truth for the actual rules is the structured [`HordeRuleset`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSummaryLine {
    /// Short category label, e.g. "Waves", "Survivor life", "Setup".
    pub label: &'static str,
    /// This deck's behavior for that category, human-readable.
    pub detail: String,
}

/// How many NON-token cards a [`WaveTermination::UntilNonToken`] wave must
/// resolve before it ends.
///
/// A plain number would cover the classic rule ("until the first nontoken"), but
/// the count genuinely varies *per Horde turn* in published decks — the Zombies
/// Horde ramps difficulty by snaking the count up and back down. Modelling it as
/// a typed schedule keeps that inside the ruleset instead of leaking a
/// special-case counter into the turn engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WaveCount {
    /// The same number of nontokens every Horde turn. `Fixed(1)` is the classic
    /// "reveal until the first nontoken card" rule.
    Fixed(u32),
    /// Ramp the count up from `min` to `max`, then back down, repeating — a
    /// triangle wave over the Horde's turns. The Zombies Horde is
    /// `Snaking { min: 1, max: 3 }`: "Wave 1, then Wave 2, and so on for Wave 3.
    /// On its next turn, it will descend back down to Wave 2, snaking back and
    /// forth" — i.e. 1, 2, 3, 2, 1, 2, 3, …
    Snaking { min: u32, max: u32 },
}

impl WaveCount {
    /// How many nontokens end the wave on the Horde's `turn_index`-th turn
    /// (0-based).
    ///
    /// `Snaking` is a triangle wave of period `2 * (max - min)`: the phase walks
    /// up to `max` and back down toward `min` without repeating the endpoints,
    /// which is what "snaking back and forth" means. A degenerate range
    /// (`max <= min`) collapses to `min`, so a malformed ruleset still yields a
    /// terminating wave rather than a divide-by-zero.
    pub fn nontokens_for_turn(self, turn_index: u32) -> u32 {
        match self {
            WaveCount::Fixed(n) => n,
            WaveCount::Snaking { min, max } => {
                if max <= min {
                    return min;
                }
                let span = max - min;
                let period = span * 2;
                let phase = turn_index % period;
                min + if phase <= span { phase } else { period - phase }
            }
        }
    }
}

/// How a single Horde turn decides how many cards to reveal-and-resolve.
///
/// This is a *policy*, not a scalar count, because the rule genuinely varies by
/// ruleset: the Theros challenge decks reveal a fixed number, the original
/// Knudson rules reveal until a non-token card, and the community format ends a
/// "wave" when a card of at least a given rarity resolves. Modelling it as a
/// typed enum lets every current and future ruleset slot in as a pure addition
/// rather than overloading a number with special-case meanings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WaveTermination {
    /// Reveal-and-resolve a fixed base number of cards each Horde turn. Runtime
    /// bonuses that depend on live game state (one extra per Horde artifact in
    /// play, one extra per additional survivor) are layered on in the turn
    /// engine, not stored here — this is only the base count (e.g. 2 for Battle
    /// the Horde, 1 for Face the Hydra).
    FixedCount(u32),
    /// Reveal-and-resolve cards until `count` NON-token cards have been cast,
    /// which ends the wave. Every token revealed along the way enters the
    /// battlefield; each nontoken is cast (for free), and once the required
    /// number have resolved the wave stops — the next revealed card stays in the
    /// library for the following Horde turn.
    ///
    /// The authentic behavior for token-heavy decks (the original Knudson rules,
    /// and the Doctor Who Cyberman Horde, which is ~2/3 tokens): a small
    /// `FixedCount` would usually reveal only tokens and barely advance the real
    /// threats, so "reveal until N nontokens" keeps the wave pressuring.
    ///
    /// `count` is a [`WaveCount`] rather than a bare number because the count is
    /// not always constant: the Zombies Horde escalates it 1 → 2 → 3 → 2 → 1 …
    /// as a difficulty ramp. `WaveCount::Fixed(1)` is the classic
    /// "until the first nontoken" rule.
    UntilNonToken { count: WaveCount },
    /// Reveal-and-resolve cards until (and including) the first card whose rarity
    /// is at least the given threshold, which ends the wave. Tokens (no rarity)
    /// and nontoken cards *below* the threshold are deployed/cast and the wave
    /// CONTINUES; the first card at or above the threshold is cast and then stops
    /// the wave, leaving the rest of the library for the next Horde turn.
    ///
    /// This is the community "waves" rule used by the D&D Horde: "a wave ends when
    /// an Uncommon, Rare, or Mythic card enters" — i.e. `UntilRarityAtLeast(
    /// Rarity::Uncommon)`. Commons and tokens are the filler that builds the board
    /// up; the first uncommon-or-better is the payoff that caps the wave. Rarity
    /// is a card-data property (`Rarity` derives `Ord`: Common < Uncommon < Rare
    /// < Mythic), not a CR concept — Horde Magic is a casual community format.
    UntilRarityAtLeast(Rarity),
}

/// How a Horde deck treats its own legendary permanent cards when they would be
/// put into the Horde's graveyard from its library (the Horde is damage-milled,
/// see [`crate::game::horde::mill_from_loss`]).
///
/// A typed axis rather than a `bool` because legendary handling has more than two
/// plausible community variants — ordinary graveyard, the "boss recurs"
/// phase-out rule below, or a future "shuffle back into the library" — and a
/// boolean could name at most one of them. Casual community format
/// (hordemagic.com advanced rules); no CR governs the axis itself, though
/// [`EtbThenPhaseOut`](Self::EtbThenPhaseOut) resolves through CR 603.6 (enters-
/// the-battlefield triggers) and CR 702.26 (phasing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HordeLegendaryDeath {
    /// Basic rules: a milled legendary is put into the graveyard like any other
    /// card. The default, and what every currently-shipped deck uses.
    #[default]
    Normal,
    /// Advanced rules (Walking Dead, Stranger Things, …): "any Legendary card
    /// that goes to the Graveyard [when the Horde is damage-milled] instead
    /// enters the battlefield, ETB triggers fire, then immediately Phases Out."
    /// Milling one of the Horde's legendaries therefore DEPLOYS it (its ETB
    /// resolves) rather than removing it, and it phases back in on the Horde's
    /// next untap (CR 702.26c) — a recurring boss instead of a wasted mill.
    EtbThenPhaseOut,
}

/// Whether — and how — the engine-scripted Horde activates its permanents'
/// activated abilities during its post-combat main phase.
///
/// A typed axis rather than a `bool` because the activation cadence has more than
/// two plausible shapes (none / once per permanent / a single best-ability per
/// turn), and a boolean could name only one. Casual community format
/// (hordemagic.com): "Card-activated abilities … occur during the post-combat
/// main phase, and only once per turn"; "Horde has infinite mana (for … activation
/// costs)." The activation itself runs through the normal CR 602 activation path
/// ([`crate::game::casting::handle_activate_ability`]); this axis only gates the
/// scripted beat that drives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HordePostCombatActivation {
    /// Basic rules: the Horde never activates abilities. The default, and what
    /// every currently-shipped deck uses.
    #[default]
    None,
    /// Advanced rules (Walking Dead, Stranger Things, …): during its post-combat
    /// main phase the Horde activates each of its permanents' non-mana activated
    /// abilities once — once per permanent per turn — paying with its infinite
    /// mana (real non-mana costs such as tap/sacrifice are still paid). Per the
    /// rule "Card-activated abilities have summoning sickness", a creature's
    /// `{T}`/`{Q}` ability stays summoning-sick the turn it enters even though the
    /// emblem's Haste lets it attack (CR 302.6) — see
    /// [`crate::game::horde`]'s `tap_ability_summoning_sick_for_horde`.
    OncePerPermanent,
}

/// The parameters that distinguish one Horde deck's rules from another, carried
/// on `FormatConfig` so a single `GameFormat::Horde` variant covers the whole
/// family without sibling format variants. All fields are typed axes, not
/// booleans-as-config where a richer type exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HordeRuleset {
    /// Which fixed library the Horde seat plays.
    pub challenge_deck: ChallengeDeck,
    /// How each Horde turn sizes its reveal-and-resolve wave.
    pub wave: WaveTermination,
    /// How many turns the survivors take before the Horde's first turn, letting
    /// them establish a board (3 in the published rules).
    pub survivor_setup_turns: u8,
    /// The survivors' shared combined life total with a SINGLE survivor — the
    /// base the per-survivor delta below adjusts from. The hordemagic community
    /// rules set this at 100 ("Player life totals are combined, shared and start
    /// at 100 life"); the Theros challenge decks use a flat shared 20.
    ///
    /// Carried here rather than read from `FormatConfig::starting_life` because
    /// base life is a per-deck-family property (community 100 vs Theros 20), and
    /// it belongs next to `per_extra_survivor_life_delta` — the two halves of the
    /// same combined-total formula. `GameState::new` computes the combined total
    /// as `combined_base_life + per_extra_survivor_life_delta * (survivors - 1)`
    /// and distributes it across the survivor seats (CR 810.9a shared team life).
    pub combined_base_life: i32,
    /// Life added to the survivors' shared combined total for each survivor
    /// beyond the first. `0` for a flat combined total (Theros: shared 20); the
    /// hordemagic community rules use −15 ("Each player reduces {15} life to the
    /// total life beyond the 1st player") — so 1/2/3 survivors → 100/85/70.
    pub per_extra_survivor_life_delta: i32,
    /// Whether the Horde's creatures are forced to attack each combat if able
    /// (true for every published Horde ruleset; a typed axis so a future ruleset
    /// can opt out — e.g. Defeat a God's revelers that only attack when told).
    pub horde_creatures_forced_attackers: bool,
    /// How the Horde's own legendary cards are treated when milled to its
    /// graveyard by damage. `Normal` for the basic community decks; the advanced
    /// decks (Walking Dead, Stranger Things, …) use
    /// [`HordeLegendaryDeath::EtbThenPhaseOut`]. `#[serde(default)]` so Horde
    /// configs serialized before this axis existed still deserialize — they
    /// predate any advanced deck, so the `Normal` back-fill is correct.
    #[serde(default)]
    pub legendary_death: HordeLegendaryDeath,
    /// Whether the Horde activates its permanents' abilities post-combat. `None`
    /// for the basic community decks; the advanced decks use
    /// [`HordePostCombatActivation::OncePerPermanent`], which also grants the
    /// Horde infinite mana (its activation costs are otherwise unpayable).
    /// `#[serde(default)]` so pre-axis serialized Horde configs still deserialize.
    #[serde(default)]
    pub post_combat_activation: HordePostCombatActivation,
}

impl HordeRuleset {
    /// Render this ruleset's typed axes into human-readable, labeled lines for
    /// the Horde deck picker and any in-game info panel.
    ///
    /// This is the single authority for turning the structured Horde rules into
    /// prose. The frontend renders these lines verbatim; it must never interpret
    /// [`WaveTermination`], the life deltas, or the setup count into its own
    /// wording, because that is game-rule interpretation and belongs in the
    /// engine. Because the summary is produced by matching over the struct's own
    /// fields, every deck's summary is complete by construction — a new
    /// advanced-rules axis added to `HordeRuleset` adds one line here and every
    /// deck picks it up, rather than requiring a hand-authored per-deck string.
    pub fn summary(&self) -> Vec<RuleSummaryLine> {
        // Small pluralizer so counts read naturally without a formatting crate.
        fn plural(n: u32) -> &'static str {
            if n == 1 {
                ""
            } else {
                "s"
            }
        }

        // The wave line is the deck's signature difference, so it leads.
        let waves = match self.wave {
            WaveTermination::FixedCount(n) => {
                format!("Reveals {n} card{} each Horde turn", plural(n))
            }
            WaveTermination::UntilNonToken {
                count: WaveCount::Fixed(1),
            } => "Reveals until the first nontoken card is cast".to_string(),
            WaveTermination::UntilNonToken {
                count: WaveCount::Fixed(n),
            } => format!("Reveals until {n} nontoken cards are cast"),
            WaveTermination::UntilNonToken {
                count: WaveCount::Snaking { min, max },
            } => format!(
                "Reveals until N nontokens are cast — N snakes {min} \u{2192} {max} \u{2192} {min} \
                 each turn"
            ),
            WaveTermination::UntilRarityAtLeast(rarity) => {
                format!(
                    "A wave ends at the first {} or better card",
                    rarity_word(rarity)
                )
            }
        };

        // Survivor life: the combined base plus the per-extra-survivor scaling.
        let life = if self.per_extra_survivor_life_delta == 0 {
            format!("{} life, shared by all survivors", self.combined_base_life)
        } else {
            format!(
                "{} life shared, {:+} per extra survivor",
                self.combined_base_life, self.per_extra_survivor_life_delta
            )
        };

        let setup = format!(
            "Survivors take {} turn{} to set up before the Horde's first turn",
            self.survivor_setup_turns,
            plural(u32::from(self.survivor_setup_turns)),
        );

        let combat = if self.horde_creatures_forced_attackers {
            "The Horde's creatures attack every combat if able".to_string()
        } else {
            "The Horde's creatures attack only when instructed".to_string()
        };

        let mut lines = vec![
            RuleSummaryLine {
                label: "Waves",
                detail: waves,
            },
            RuleSummaryLine {
                label: "Survivor life",
                detail: life,
            },
            RuleSummaryLine {
                label: "Setup",
                detail: setup,
            },
            RuleSummaryLine {
                label: "Horde combat",
                detail: combat,
            },
        ];

        // The legendary line is an *advanced-rules* axis: only the decks that opt
        // into the "boss recurs" rule surface it, so a basic deck's summary stays
        // exactly the four lines above (and byte-identical to before this axis
        // existed). This is a distinguishing rule, not the vanilla default, so
        // showing "legendaries die normally" for every basic deck would be noise.
        match self.legendary_death {
            HordeLegendaryDeath::Normal => {}
            HordeLegendaryDeath::EtbThenPhaseOut => lines.push(RuleSummaryLine {
                label: "Legendary deaths",
                detail: "A milled legendary enters, triggers its ETB, then phases out — \
                         it returns on the Horde's next untap instead of being removed"
                    .to_string(),
            }),
        }

        // Post-combat activation is likewise an advanced-only axis — surfaced only
        // for the decks that opt in, so basic decks keep their prior line set.
        match self.post_combat_activation {
            HordePostCombatActivation::None => {}
            HordePostCombatActivation::OncePerPermanent => lines.push(RuleSummaryLine {
                label: "Post-combat",
                detail: "After combat the Horde activates each of its permanents' abilities \
                         once, with infinite mana"
                    .to_string(),
            }),
        }

        lines
    }
}

/// Lowercase English word for a rarity, for the Horde wave-rule summary
/// ("uncommon or better"). Horde Magic's community wave rule is keyed on rarity
/// order (`Rarity` derives `Ord`); this is display text, not a CR concept.
fn rarity_word(rarity: Rarity) -> &'static str {
    match rarity {
        Rarity::Common => "common",
        Rarity::Uncommon => "uncommon",
        Rarity::Rare => "rare",
        Rarity::Mythic => "mythic",
        Rarity::Special => "special",
        Rarity::Bonus => "bonus",
    }
}

impl ChallengeDeck {
    /// Every selectable challenge deck, in display order.
    ///
    /// New decks must be appended here as well as given a [`Self::metadata`] arm
    /// (that match is exhaustive, so the compiler catches a missing arm; the
    /// `registry_covers_every_challenge_deck` test catches a missing entry here).
    pub const ALL: &'static [ChallengeDeck] = &[
        ChallengeDeck::CybermanHorde,
        ChallengeDeck::DndHorde,
        ChallengeDeck::ZombiesHorde,
        ChallengeDeck::SliversHorde,
        ChallengeDeck::HumansGodzillaHorde,
    ];

    /// Display metadata for a single deck. Exhaustive match: adding a
    /// `ChallengeDeck` variant fails to compile until it is described here.
    ///
    /// The flavor `label`/`short_label`/`description` are per-deck display text;
    /// the structured `default_ruleset` and its rendered `rules` summary are the
    /// single source of truth for the actual rules, so they are attached once
    /// from [`Self::default_ruleset`] rather than restated per arm.
    pub fn metadata(self) -> ChallengeDeckMetadata {
        let (label, short_label, description) = match self {
            ChallengeDeck::CybermanHorde => (
                "Cyberman Horde",
                "CYB",
                "Doctor Who — Cybermen and Daleks swarm in waves that \
                 run until a nontoken card is cast",
            ),
            ChallengeDeck::DndHorde => (
                "D&D Horde — Oozes",
                "DND",
                "Dungeons & Dragons — a self-replicating Ooze swarm; \
                 each wave ends when an uncommon or better is revealed",
            ),
            ChallengeDeck::ZombiesHorde => (
                "Zombies Horde",
                "ZOM",
                "An undead swarm whose waves ramp 1 → 2 → 3 nontokens \
                 and back down, snaking between pressure and respite",
            ),
            ChallengeDeck::SliversHorde => (
                "Slivers Horde",
                "SLV",
                "Every Sliver buffs every other Sliver — the swarm \
                 compounds, growing stronger as it grows wider",
            ),
            ChallengeDeck::HumansGodzillaHorde => (
                "Humans & Godzilla Horde",
                "HGZ",
                "A wide, cheap human army punctuated by a handful of \
                 enormous Godzilla-series titans",
            ),
        };
        let default_ruleset = self.default_ruleset();
        let rules = default_ruleset.summary();
        ChallengeDeckMetadata {
            deck: self,
            label,
            short_label,
            description,
            default_ruleset,
            rules,
        }
    }

    /// The full catalog of selectable Horde decks with display metadata. The
    /// frontend's Horde deck picker renders this directly — it is the single
    /// source of truth for which decks exist and how they are labeled.
    pub fn registry() -> Vec<ChallengeDeckMetadata> {
        Self::ALL.iter().copied().map(Self::metadata).collect()
    }

    /// The canonical ruleset for each Horde deck. Centralizes deck-identity →
    /// rules so the registry, `FormatConfig::for_format`, and callers that only
    /// have the deck enum agree on one source of truth.
    pub fn default_ruleset(self) -> HordeRuleset {
        match self {
            ChallengeDeck::CybermanHorde => HordeRuleset {
                challenge_deck: ChallengeDeck::CybermanHorde,
                // The Cyberman deck is ~2/3 tokens, so a small `FixedCount`
                // would usually reveal only tokens and barely advance the real
                // threats. "Reveal until a non-token card" is the authentic
                // token-heavy behavior for this deck — the classic single-
                // nontoken rule, i.e. `Fixed(1)`.
                wave: WaveTermination::UntilNonToken {
                    count: WaveCount::Fixed(1),
                },
                survivor_setup_turns: 3,
                combined_base_life: 100,
                per_extra_survivor_life_delta: -15,
                horde_creatures_forced_attackers: true,
                // All shipped community decks are basic decks — the advanced
                // legendary and post-combat rules arrive with Walking Dead /
                // Stranger Things.
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            },
            ChallengeDeck::DndHorde => HordeRuleset {
                challenge_deck: ChallengeDeck::DndHorde,
                // The D&D Horde's defining rule: a wave ends when an Uncommon,
                // Rare, or Mythic card enters (commons and tokens keep it going).
                wave: WaveTermination::UntilRarityAtLeast(Rarity::Uncommon),
                // No published D&D-specific setup/life values; follow the generic
                // hordemagic rules, mirroring the Cyberman defaults (tunable axes).
                survivor_setup_turns: 3,
                combined_base_life: 100,
                per_extra_survivor_life_delta: -15,
                horde_creatures_forced_attackers: true,
                // All shipped community decks are basic decks — the advanced
                // legendary and post-combat rules arrive with Walking Dead /
                // Stranger Things.
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            },
            ChallengeDeck::ZombiesHorde => HordeRuleset {
                challenge_deck: ChallengeDeck::ZombiesHorde,
                // The deck's defining rule: "it will Wave 1, putting out Tokens
                // until it flips its first nontoken card. Then ... Wave 2 ... and
                // so on for Wave 3. On its next turn, it will descend back down
                // to Wave 2, snaking back and forth."
                wave: WaveTermination::UntilNonToken {
                    count: WaveCount::Snaking { min: 1, max: 3 },
                },
                // No published Zombies-specific setup/life values; follow the
                // generic hordemagic rules, as the other community decks do.
                survivor_setup_turns: 3,
                combined_base_life: 100,
                per_extra_survivor_life_delta: -15,
                horde_creatures_forced_attackers: true,
                // All shipped community decks are basic decks — the advanced
                // legendary and post-combat rules arrive with Walking Dead /
                // Stranger Things.
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            },
            ChallengeDeck::SliversHorde => HordeRuleset {
                challenge_deck: ChallengeDeck::SliversHorde,
                // States no deck-specific rules; takes the hordemagic BASIC rule
                // verbatim: "Waves end when an UNCOMMON, RARE or MYTHIC card is
                // cast."
                wave: WaveTermination::UntilRarityAtLeast(Rarity::Uncommon),
                // Basic rules: "Each player takes {3} consecutive turns to set up
                // their board."
                survivor_setup_turns: 3,
                combined_base_life: 100,
                per_extra_survivor_life_delta: -15,
                horde_creatures_forced_attackers: true,
                // All shipped community decks are basic decks — the advanced
                // legendary and post-combat rules arrive with Walking Dead /
                // Stranger Things.
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            },
            ChallengeDeck::HumansGodzillaHorde => HordeRuleset {
                challenge_deck: ChallengeDeck::HumansGodzillaHorde,
                // As above — no deck-specific rules stated, so the basic
                // community wave rule applies.
                wave: WaveTermination::UntilRarityAtLeast(Rarity::Uncommon),
                survivor_setup_turns: 3,
                combined_base_life: 100,
                per_extra_survivor_life_delta: -15,
                horde_creatures_forced_attackers: true,
                // All shipped community decks are basic decks — the advanced
                // legendary and post-combat rules arrive with Walking Dead /
                // Stranger Things.
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            },
        }
    }
}

/// Configuration for a game format, describing player counts, starting life, deck rules, etc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatConfig {
    pub format: GameFormat,
    pub starting_life: i32,
    pub min_players: u8,
    pub max_players: u8,
    pub deck_size: u16,
    pub singleton: bool,
    pub command_zone: bool,
    pub commander_damage_threshold: Option<u8>,
    pub range_of_influence: Option<u8>,
    pub team_based: bool,
    /// CR 904.2a / CR 904.6: In default Archenemy, the single-player team is
    /// designated as the archenemy and takes the first turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archenemy_player: Option<PlayerId>,
    /// Engine-derived predicate: true when the format uses a commander card
    /// and the commander-damage state-based action (CR 903.10a / CR 704.5u).
    /// Covers Commander, Duel Commander, Pauper Commander, Brawl, and
    /// Historic Brawl. The frontend consumes this directly — it must never
    /// re-list commander-style formats client-side.
    pub uses_commander: bool,
    /// Engine-derived predicate (mirrors `GameFormat::supplies_fixed_deck`):
    /// true when the format's deck is fixed and supplied automatically by the
    /// engine, so the player builds/selects nothing. True only for Momir's
    /// Madness. The frontend consumes this directly to bypass deck-selection
    /// gates — it must never re-list fixed-deck formats client-side.
    #[serde(default)]
    pub supplies_fixed_deck: bool,
    /// Capability flag: when true, the server (and other transport gates)
    /// permit `GameAction::Debug(_)` from any player in this session. Off by
    /// default. Orthogonal to format — a sandbox Commander game plays
    /// exactly like a normal Commander game with one additional permission.
    /// Immutable for the life of the session.
    #[serde(default)]
    pub allow_debug_actions: bool,
    /// Present only for `GameFormat::Horde`: the per-deck Horde rules (which
    /// challenge deck, wave sizing, setup turns, life scaling, forced attackers).
    /// `None` for every other format. Mirrors the optional `archenemy_player`
    /// pattern so existing serialized configs deserialize unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horde_ruleset: Option<HordeRuleset>,
}

impl GameFormat {
    /// Maps a playable game format to its corresponding legality format for card pool validation.
    /// Returns `None` for formats that don't restrict card pools (FreeForAll, TwoHeadedGiant).
    pub fn legality_format(self) -> Option<LegalityFormat> {
        match self {
            GameFormat::Standard => Some(LegalityFormat::Standard),
            GameFormat::Commander => Some(LegalityFormat::Commander),
            GameFormat::Pioneer => Some(LegalityFormat::Pioneer),
            GameFormat::Modern => Some(LegalityFormat::Modern),
            GameFormat::Premodern => Some(LegalityFormat::Premodern),
            GameFormat::Legacy => Some(LegalityFormat::Legacy),
            GameFormat::Vintage => Some(LegalityFormat::Vintage),
            GameFormat::Historic => Some(LegalityFormat::Historic),
            GameFormat::Timeless => Some(LegalityFormat::Timeless),
            GameFormat::Pauper => Some(LegalityFormat::Pauper),
            GameFormat::PauperCommander => Some(LegalityFormat::PauperCommander),
            GameFormat::DuelCommander => Some(LegalityFormat::DuelCommander),
            GameFormat::Brawl => Some(LegalityFormat::StandardBrawl),
            GameFormat::HistoricBrawl => Some(LegalityFormat::Brawl),
            GameFormat::TinyLeaders
            | GameFormat::Oathbreaker
            | GameFormat::FreeForAll
            | GameFormat::TwoHeadedGiant
            | GameFormat::Archenemy
            | GameFormat::Planechase
            // Momir's pool is the entire creature corpus — no legality restriction.
            | GameFormat::Momir
            // Horde uses engine-supplied fixed decks (Horde library + survivor
            // precons) — no constructed legality restriction.
            | GameFormat::Horde
            | GameFormat::Limited => None,
        }
    }

    /// CR 100.4a: Per-format sideboard policy.
    ///
    /// Returns `Forbidden` for Commander/Brawl/Historic Brawl (no sideboard),
    /// `Limited(15)` for constructed formats, and `Unlimited` for casual
    /// multiplayer variants that impose no size cap.
    pub fn sideboard_policy(self) -> SideboardPolicy {
        match self {
            GameFormat::Standard
            | GameFormat::Pioneer
            | GameFormat::Modern
            | GameFormat::Premodern
            | GameFormat::Legacy
            | GameFormat::Vintage
            | GameFormat::Historic
            | GameFormat::Timeless
            | GameFormat::Pauper => SideboardPolicy::Limited(15),
            GameFormat::Commander
            | GameFormat::PauperCommander
            | GameFormat::DuelCommander
            | GameFormat::Oathbreaker
            | GameFormat::Brawl
            // Momir has no sideboard — the deck is exactly 60 snow basic lands.
            | GameFormat::Momir
            // Horde decks are engine-supplied and fixed — no sideboard.
            | GameFormat::Horde
            | GameFormat::HistoricBrawl => SideboardPolicy::Forbidden,
            GameFormat::TinyLeaders => SideboardPolicy::Limited(10),
            GameFormat::FreeForAll
            | GameFormat::TwoHeadedGiant
            | GameFormat::Archenemy
            | GameFormat::Planechase
            | GameFormat::Limited => SideboardPolicy::Unlimited,
        }
    }

    /// Whether this format grants a free first mulligan in duels (2-player
    /// games). Combines CR 103.5c (which covers Brawl and all multiplayer
    /// games) with the Commander Rules Committee's supplementary rule (which
    /// extends free-first-mulligan to Commander and Historic Brawl duels).
    ///
    /// Multiplayer games (3+ seats) always get the free first mulligan per
    /// CR 103.5c regardless of format; this predicate is the *duel* override.
    pub fn grants_free_first_mulligan(self) -> bool {
        matches!(
            self,
            GameFormat::Commander
                | GameFormat::PauperCommander
                | GameFormat::DuelCommander
                | GameFormat::Oathbreaker
                | GameFormat::Brawl
                | GameFormat::HistoricBrawl,
        )
    }

    /// Whether this format uses a commander card and the commander-damage
    /// state-based action (CR 903.10a / CR 704.5u). True for Commander, Duel
    /// Commander, Pauper Commander, Brawl, and Historic Brawl — every format
    /// whose `FormatConfig` has both `command_zone: true` and a non-`None`
    /// `commander_damage_threshold`. The frontend consumes the derived
    /// `FormatConfig::uses_commander` field rather than re-listing the
    /// commander-style variants client-side.
    pub fn uses_commander(self) -> bool {
        matches!(
            self,
            GameFormat::Commander
                | GameFormat::DuelCommander
                | GameFormat::PauperCommander
                | GameFormat::Brawl
                | GameFormat::HistoricBrawl,
        )
    }

    /// Whether this format's deck is fixed by the format rules and supplied
    /// automatically by the engine — the player never builds or selects one.
    /// True only for Momir's Madness, whose deck is the fixed 60-card snow-basic
    /// list (`deck_loading::momir_fixed_deck_names`); `load_and_hydrate_decks`
    /// synthesizes it for every seat. The frontend consumes the derived
    /// `FormatConfig::supplies_fixed_deck` field to bypass deck-selection gates,
    /// and must never re-list fixed-deck formats client-side.
    pub fn supplies_fixed_deck(self) -> bool {
        // Horde is intentionally excluded: only the Horde *seat* plays an
        // engine-supplied library (injected like Archenemy's shared scheme
        // deck), while the survivors — the human players — still build and
        // select their own decks. This predicate means "no player selects a
        // deck", which is true only for Momir.
        matches!(self, GameFormat::Momir)
    }

    /// Display label for validation error messages (e.g., "Not Pioneer legal").
    pub fn label(self) -> &'static str {
        match self {
            GameFormat::Standard => "Standard",
            GameFormat::Limited => "Limited",
            GameFormat::Commander => "Commander",
            GameFormat::Pioneer => "Pioneer",
            GameFormat::Modern => "Modern",
            GameFormat::Premodern => "Premodern",
            GameFormat::Legacy => "Legacy",
            GameFormat::Vintage => "Vintage",
            GameFormat::Historic => "Historic",
            GameFormat::Timeless => "Timeless",
            GameFormat::Pauper => "Pauper",
            GameFormat::PauperCommander => "Pauper Commander",
            GameFormat::DuelCommander => "Duel Commander",
            GameFormat::TinyLeaders => "Tiny Leaders: Reborn",
            GameFormat::Oathbreaker => "Oathbreaker",
            GameFormat::Brawl => "Brawl",
            GameFormat::HistoricBrawl => "Historic Brawl",
            GameFormat::FreeForAll => "Free-for-All",
            GameFormat::TwoHeadedGiant => "Two-Headed Giant",
            GameFormat::Archenemy => "Archenemy",
            GameFormat::Planechase => "Planechase",
            GameFormat::Momir => "Momir's Madness",
            GameFormat::Horde => "Horde",
        }
    }

    /// Authoritative list of user-selectable formats. The frontend consumes
    /// this (via the `get_format_registry` WASM export) to render format
    /// pickers, default configs, and badges. Surface-specific callers may
    /// filter this list when a format is not appropriate for that entry point
    /// (for example deck-construction or solo-AI setup).
    pub fn registry() -> Vec<FormatMetadata> {
        vec![
            FormatMetadata {
                format: GameFormat::Standard,
                label: "Standard",
                short_label: "STD",
                description: "Rotating card pool",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::standard(),
            },
            FormatMetadata {
                format: GameFormat::Pioneer,
                label: "Pioneer",
                short_label: "PIO",
                description: "Non-rotating from 2012",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::pioneer(),
            },
            FormatMetadata {
                format: GameFormat::Modern,
                label: "Modern",
                short_label: "MOD",
                description: "Non-rotating from Mirrodin onward",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::modern(),
            },
            FormatMetadata {
                format: GameFormat::Premodern,
                label: "Premodern",
                short_label: "PRE",
                description: "Old-frame constructed through Scourge",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::premodern(),
            },
            FormatMetadata {
                format: GameFormat::Legacy,
                label: "Legacy",
                short_label: "LEG",
                description: "Eternal format, all sets legal",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::legacy(),
            },
            FormatMetadata {
                format: GameFormat::Vintage,
                label: "Vintage",
                short_label: "VIN",
                description: "Broadest pool, Power Nine restricted",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::vintage(),
            },
            FormatMetadata {
                format: GameFormat::Historic,
                label: "Historic",
                short_label: "HIS",
                description: "Arena's eternal format",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::historic(),
            },
            FormatMetadata {
                format: GameFormat::Timeless,
                label: "Timeless",
                short_label: "TML",
                description: "Arena's eternal non-rotating format",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::timeless(),
            },
            FormatMetadata {
                format: GameFormat::Pauper,
                label: "Pauper",
                short_label: "PAU",
                description: "Commons only",
                group: FormatGroup::Constructed,
                default_config: FormatConfig::pauper(),
            },
            FormatMetadata {
                format: GameFormat::Commander,
                label: "Commander",
                short_label: "CMD",
                description: "100-card singleton, 2\u{2013}4 players",
                group: FormatGroup::Commander,
                default_config: FormatConfig::commander(),
            },
            FormatMetadata {
                format: GameFormat::DuelCommander,
                label: "Duel Commander",
                short_label: "DUC",
                description: "Tournament 1v1 Commander, 30 life",
                group: FormatGroup::Commander,
                default_config: FormatConfig::duel_commander(),
            },
            FormatMetadata {
                format: GameFormat::PauperCommander,
                label: "Pauper Commander",
                short_label: "PDH",
                description: "Commons-only singleton Commander",
                group: FormatGroup::Commander,
                default_config: FormatConfig::pauper_commander(),
            },
            FormatMetadata {
                format: GameFormat::TinyLeaders,
                label: "Tiny Leaders: Reborn",
                short_label: "TLR",
                description: "50-card Tiny singleton",
                group: FormatGroup::Commander,
                default_config: FormatConfig::tiny_leaders(),
            },
            FormatMetadata {
                format: GameFormat::Oathbreaker,
                label: "Oathbreaker",
                short_label: "OBK",
                description: "60-card singleton, Planeswalker + signature spell",
                group: FormatGroup::Commander,
                default_config: FormatConfig::oathbreaker(),
            },
            FormatMetadata {
                format: GameFormat::Brawl,
                label: "Brawl",
                short_label: "BRL",
                description: "60-card Standard singleton",
                group: FormatGroup::Commander,
                default_config: FormatConfig::brawl(),
            },
            FormatMetadata {
                format: GameFormat::HistoricBrawl,
                label: "Historic Brawl",
                short_label: "HBR",
                description: "60-card eternal singleton",
                group: FormatGroup::Commander,
                default_config: FormatConfig::historic_brawl(),
            },
            FormatMetadata {
                format: GameFormat::FreeForAll,
                label: "Free-for-All",
                short_label: "FFA",
                description: "3\u{2013}6 player battle royale",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::free_for_all(),
            },
            FormatMetadata {
                format: GameFormat::TwoHeadedGiant,
                label: "Two-Headed Giant",
                short_label: "2HG",
                description: "4 players, two teams of two",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::two_headed_giant(),
            },
            FormatMetadata {
                format: GameFormat::Archenemy,
                label: "Archenemy",
                short_label: "ARC",
                description: "One archenemy against a team of heroes",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::archenemy(),
            },
            FormatMetadata {
                format: GameFormat::Planechase,
                label: "Planechase",
                short_label: "PLC",
                description: "60-card multiplayer with a communal planar deck",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::planechase(),
            },
            FormatMetadata {
                format: GameFormat::Limited,
                label: "Limited",
                short_label: "LIM",
                description: "Draft or sealed, 40-card deck",
                group: FormatGroup::Limited,
                default_config: FormatConfig::limited(),
            },
            FormatMetadata {
                format: GameFormat::Momir,
                label: "Momir's Madness",
                short_label: "MOM",
                description: "60 snow basic lands, random creature tokens",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::momir(),
            },
            FormatMetadata {
                format: GameFormat::Horde,
                label: "Horde",
                short_label: "HRD",
                description: "Co-op team vs a self-piloting Horde deck",
                group: FormatGroup::Multiplayer,
                default_config: FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset()),
            },
        ]
    }
}

impl FormatConfig {
    pub fn topology(&self) -> FormatTopology {
        match self.format {
            GameFormat::TwoHeadedGiant => FormatTopology::FixedTeams {
                team_size: 2,
                team_count: 2,
                turn_structure: TurnStructure::SharedTeamTurns,
            },
            GameFormat::Archenemy => FormatTopology::OneVsMany {
                archenemy: self.archenemy_player.unwrap_or(PlayerId(0)),
                turn_structure: TurnStructure::SharedTeamTurns,
            },
            // Horde reuses the one-vs-many topology with the Horde as the
            // "archenemy" seat and the survivors as a shared-turn team. The
            // survivors-first turn order and no-life-total handling are applied
            // in `starting_player`/`starting_life_for_player`, not here.
            GameFormat::Horde => FormatTopology::OneVsMany {
                archenemy: self.archenemy_player.unwrap_or(PlayerId(0)),
                turn_structure: TurnStructure::SharedTeamTurns,
            },
            _ if self.team_based => FormatTopology::FixedTeams {
                team_size: 2,
                team_count: 2,
                turn_structure: TurnStructure::SharedTeamTurns,
            },
            _ => FormatTopology::IndividualSeats,
        }
    }

    pub fn starting_life_for_seat(&self) -> i32 {
        match self.topology() {
            FormatTopology::IndividualSeats => self.starting_life,
            FormatTopology::FixedTeams { team_size, .. } => {
                self.starting_life / i32::from(team_size)
            }
            FormatTopology::OneVsMany { .. } => self.starting_life,
        }
    }

    pub fn starting_life_for_player(&self, player: PlayerId) -> i32 {
        match self.topology() {
            FormatTopology::IndividualSeats => self.starting_life,
            FormatTopology::FixedTeams { team_size, .. } => {
                self.starting_life / i32::from(team_size)
            }
            // Horde and Archenemy both map to OneVsMany, so the format must be
            // distinguished here.
            FormatTopology::OneVsMany { archenemy, .. } => {
                if self.format == GameFormat::Horde {
                    // The Horde has no life total (it loses by decking out, not
                    // by life), so its own seat's life value is never consulted.
                    // Every seat is seeded here with the survivors' single-
                    // survivor combined base life, and that combined total is
                    // distributed across the survivor seats in `GameState::new`
                    // once the seat count is known (CR 810.9a reads each seat
                    // through the shared team total). The base lives on the
                    // ruleset (per-deck: community 100, Theros 20); fall back to
                    // the format-wide `starting_life` only if a Horde config was
                    // somehow built without a ruleset. The Horde is NOT given the
                    // archenemy's 40 life.
                    self.horde_ruleset
                        .as_ref()
                        .map_or(self.starting_life, |r| r.combined_base_life)
                } else if player == archenemy {
                    // CR 904.5: The archenemy starts at 40 life; each other
                    // player starts at 20. This is not a shared life total.
                    40
                } else {
                    20
                }
            }
        }
    }

    pub fn archenemy_player(&self) -> Option<PlayerId> {
        match self.topology() {
            FormatTopology::OneVsMany { archenemy, .. } => Some(archenemy),
            FormatTopology::IndividualSeats | FormatTopology::FixedTeams { .. } => None,
        }
    }

    pub fn validate_for_player_count(&self, player_count: u8) -> Result<(), String> {
        // Both Archenemy and Horde designate a single special seat via
        // `archenemy_player`, which must be a valid seat index.
        if matches!(self.format, GameFormat::Archenemy | GameFormat::Horde) {
            let archenemy = self.archenemy_player().unwrap_or(PlayerId(0));
            if archenemy.0 >= player_count {
                let seat = if self.format == GameFormat::Horde {
                    "horde"
                } else {
                    "archenemy"
                };
                return Err(format!(
                    "{seat}_player must be less than player_count ({player_count})"
                ));
            }
        }
        Ok(())
    }

    pub fn starting_player(&self) -> PlayerId {
        // Horde: the survivors set up their board first (the published rules
        // give them several turns before the Horde's first turn), so the first
        // turn goes to a survivor, NOT the Horde. The Horde occupies the
        // `archenemy_player` seat; the first survivor is the lowest seat index
        // that isn't the Horde. `min_players` (2) guarantees such a seat exists.
        if self.format == GameFormat::Horde {
            let horde = self.archenemy_player().unwrap_or(PlayerId(0));
            return PlayerId(if horde.0 == 0 { 1 } else { 0 });
        }
        // CR 904.6: The archenemy takes the first turn instead of a randomly
        // determined player. Non-Archenemy formats keep the legacy default.
        self.archenemy_player().unwrap_or(PlayerId(0))
    }

    pub fn standard() -> Self {
        FormatConfig {
            format: GameFormat::Standard,
            starting_life: 20,
            min_players: 2,
            max_players: 2,
            deck_size: 60,
            singleton: false,
            command_zone: false,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    pub fn commander() -> Self {
        FormatConfig {
            format: GameFormat::Commander,
            starting_life: 40,
            min_players: 2,
            max_players: 6,
            deck_size: 100,
            singleton: true,
            command_zone: true,
            commander_damage_threshold: Some(21),
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: true,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    pub fn pioneer() -> Self {
        FormatConfig {
            format: GameFormat::Pioneer,
            ..Self::standard()
        }
    }

    /// Modern: non-rotating constructed from Mirrodin (2003) onward.
    pub fn modern() -> Self {
        FormatConfig {
            format: GameFormat::Modern,
            ..Self::standard()
        }
    }

    /// Premodern: community-maintained old-frame constructed through Scourge.
    pub fn premodern() -> Self {
        FormatConfig {
            format: GameFormat::Premodern,
            ..Self::standard()
        }
    }

    /// Legacy: non-rotating constructed spanning the full Magic card pool,
    /// minus the Legacy banned list.
    pub fn legacy() -> Self {
        FormatConfig {
            format: GameFormat::Legacy,
            ..Self::standard()
        }
    }

    /// Vintage: non-rotating constructed with the broadest legal pool,
    /// restricted rather than fully banned for Power Nine and similar.
    pub fn vintage() -> Self {
        FormatConfig {
            format: GameFormat::Vintage,
            ..Self::standard()
        }
    }

    /// Timeless: Arena's eternal non-rotating format, 60-card constructed.
    pub fn timeless() -> Self {
        FormatConfig {
            format: GameFormat::Timeless,
            ..Self::standard()
        }
    }

    /// Pauper Commander: 100-card singleton commander format restricted to
    /// commons (with an uncommon creature/planeswalker commander). Shares
    /// Commander's structural rules (life, command zone, damage threshold).
    pub fn pauper_commander() -> Self {
        FormatConfig {
            format: GameFormat::PauperCommander,
            ..Self::commander()
        }
    }

    /// Duel Commander: tournament 1v1 commander. 100-card singleton but 30
    /// life, strict duel cap, distinct banned list from regular Commander.
    pub fn duel_commander() -> Self {
        FormatConfig {
            format: GameFormat::DuelCommander,
            starting_life: 30,
            max_players: 2,
            ..Self::commander()
        }
    }

    /// Tiny Leaders: Reborn: 50-card singleton command-zone format, 20 life,
    /// no commander-damage loss threshold, and up to 10 sideboard cards.
    pub fn tiny_leaders() -> Self {
        FormatConfig {
            format: GameFormat::TinyLeaders,
            starting_life: 20,
            min_players: 2,
            max_players: 2,
            deck_size: 50,
            singleton: true,
            command_zone: true,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Oathbreaker RC: 60-card singleton, one legendary Planeswalker as the
    /// Oathbreaker commander plus one signature spell (instant/sorcery within
    /// color identity), both in the command zone. 20 life, 2–4 players,
    /// no commander-damage threshold.
    pub fn oathbreaker() -> Self {
        FormatConfig {
            format: GameFormat::Oathbreaker,
            starting_life: 20,
            min_players: 2,
            max_players: 4,
            deck_size: 60,
            singleton: true,
            command_zone: true,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Historic: non-rotating constructed using the Arena Historic card pool.
    pub fn historic() -> Self {
        FormatConfig {
            format: GameFormat::Historic,
            ..Self::standard()
        }
    }

    pub fn pauper() -> Self {
        FormatConfig {
            format: GameFormat::Pauper,
            ..Self::standard()
        }
    }

    /// Brawl: 60-card singleton with a commander, 25 starting life.
    /// Uses Standard-legal card pool (CR 903 variant for Brawl).
    pub fn brawl() -> Self {
        FormatConfig {
            format: GameFormat::Brawl,
            starting_life: 25,
            min_players: 2,
            max_players: 2,
            deck_size: 60,
            singleton: true,
            command_zone: true,
            commander_damage_threshold: Some(21),
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: true,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Historic Brawl: same rules as Brawl but with the broader Historic card pool.
    pub fn historic_brawl() -> Self {
        FormatConfig {
            format: GameFormat::HistoricBrawl,
            ..Self::brawl()
        }
    }

    pub fn free_for_all() -> Self {
        FormatConfig {
            format: GameFormat::FreeForAll,
            starting_life: 20,
            min_players: 2,
            max_players: 6,
            deck_size: 60,
            singleton: false,
            command_zone: false,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Limited: 40-card minimum, 20 starting life, 2-player, no singleton,
    /// no command zone. Used by all Draft variants.
    pub fn limited() -> Self {
        FormatConfig {
            format: GameFormat::Limited,
            starting_life: 20,
            min_players: 2,
            max_players: 2,
            deck_size: 40,
            singleton: false,
            command_zone: false,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Momir's Madness: 60 snow basic lands (12 each of Snow-Covered Plains/
    /// Island/Swamp/Mountain/Forest, no Snow-Covered Wastes), 20 life, 2-player.
    /// A game-start command-zone emblem grants the random-creature-token
    /// activated ability. No sideboard, no commander. `command_zone: true` so
    /// the command-zone activation surface and pool rehydration are enabled.
    pub fn momir() -> Self {
        FormatConfig {
            format: GameFormat::Momir,
            starting_life: 20,
            min_players: 2,
            max_players: 2,
            deck_size: 60,
            singleton: false,
            command_zone: true,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: true,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    pub fn two_headed_giant() -> Self {
        FormatConfig {
            format: GameFormat::TwoHeadedGiant,
            starting_life: 30,
            min_players: 4,
            max_players: 4,
            deck_size: 60,
            singleton: false,
            command_zone: false,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: true,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// CR 901.15a: Planechase with one communal planar deck. Player decks use
    /// normal 60-card construction; the supplementary planar deck is validated
    /// separately against the actual player count.
    pub fn planechase() -> Self {
        FormatConfig {
            format: GameFormat::Planechase,
            starting_life: 20,
            min_players: 2,
            max_players: 4,
            deck_size: 60,
            singleton: false,
            command_zone: false,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: None,
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// CR 904.1-904.11: Default Archenemy, not Supervillain Rumble (CR 904.12)
    /// and not Archenemy Commander (CR 904.13).
    pub fn archenemy() -> Self {
        FormatConfig {
            format: GameFormat::Archenemy,
            starting_life: 20,
            min_players: 2,
            max_players: 6,
            deck_size: 60,
            singleton: false,
            command_zone: true,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            archenemy_player: Some(PlayerId(0)),
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: None,
        }
    }

    /// Horde Magic (casual community variant, not CR-sanctioned). A team of
    /// survivors shares a life total and faces the self-piloting Horde seat,
    /// which occupies the `archenemy_player` seat and reuses the one-vs-many
    /// shared-turn topology. `command_zone: true` so the game-start Horde emblem
    /// (forced attackers + haste on Horde creatures) has a home, mirroring how
    /// Archenemy/Momir install their game-start command-zone objects.
    ///
    /// `starting_life` is the survivors' *combined* starting total (distributed
    /// across survivor seats at game start); the Horde itself has no life total.
    /// `supplies_fixed_deck` is `false` — only the Horde seat's library is
    /// engine-supplied (injected like the Archenemy scheme deck); survivors
    /// build and select their own decks.
    pub fn horde(ruleset: HordeRuleset) -> Self {
        FormatConfig {
            format: GameFormat::Horde,
            // Fallback only. The authoritative survivors' combined base life is
            // `HordeRuleset::combined_base_life` (per-deck: community 100, Theros
            // 20); `starting_life_for_player` and `GameState::new` read the
            // ruleset and only fall back to this if a Horde config were built
            // without one. Kept at the community base so that fallback is sane.
            starting_life: 100,
            // 1 survivor + the Horde seat, up to 4 survivors + the Horde.
            min_players: 2,
            max_players: 5,
            deck_size: 60,
            singleton: false,
            command_zone: true,
            commander_damage_threshold: None,
            range_of_influence: None,
            team_based: false,
            // The Horde occupies seat 0; survivors take the remaining seats and
            // act first (see `starting_player`).
            archenemy_player: Some(PlayerId(0)),
            uses_commander: false,
            supplies_fixed_deck: false,
            allow_debug_actions: false,
            horde_ruleset: Some(ruleset),
        }
    }

    /// Return a copy of this config with the sandbox capability enabled.
    /// Pure data transform; the resulting config is otherwise identical and
    /// keeps the same `GameFormat`, deck/seat/life rules, etc. Idempotent.
    pub fn with_sandbox(mut self) -> Self {
        self.allow_debug_actions = true;
        self
    }

    /// Default `FormatConfig` for a given `GameFormat`. Used by callers that
    /// only retain the format enum (e.g. the lobby broker) and need a full
    /// config to hand back to clients for deck-legality UX. Customizations a
    /// host may have applied on top of the default (e.g. non-standard player
    /// counts for Commander) are intentionally not recovered — guests use
    /// this purely to filter their local deck picker, and the host's own
    /// FormatConfig remains authoritative once the P2P session is established.
    pub fn for_format(format: GameFormat) -> Self {
        match format {
            GameFormat::Standard => Self::standard(),
            GameFormat::Limited => Self::limited(),
            GameFormat::Commander => Self::commander(),
            GameFormat::Pioneer => Self::pioneer(),
            GameFormat::Modern => Self::modern(),
            GameFormat::Premodern => Self::premodern(),
            GameFormat::Legacy => Self::legacy(),
            GameFormat::Vintage => Self::vintage(),
            GameFormat::Historic => Self::historic(),
            GameFormat::Timeless => Self::timeless(),
            GameFormat::Pauper => Self::pauper(),
            GameFormat::PauperCommander => Self::pauper_commander(),
            GameFormat::DuelCommander => Self::duel_commander(),
            GameFormat::TinyLeaders => Self::tiny_leaders(),
            GameFormat::Oathbreaker => Self::oathbreaker(),
            GameFormat::Brawl => Self::brawl(),
            GameFormat::HistoricBrawl => Self::historic_brawl(),
            GameFormat::FreeForAll => Self::free_for_all(),
            GameFormat::TwoHeadedGiant => Self::two_headed_giant(),
            GameFormat::Archenemy => Self::archenemy(),
            GameFormat::Planechase => Self::planechase(),
            GameFormat::Momir => Self::momir(),
            GameFormat::Horde => Self::horde(ChallengeDeck::CybermanHorde.default_ruleset()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_config_standard() {
        let config = FormatConfig::standard();
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 2);
        assert_eq!(config.deck_size, 60);
        assert!(!config.singleton);
        assert!(!config.command_zone);
        assert_eq!(config.commander_damage_threshold, None);
        assert!(!config.team_based);
    }

    #[test]
    fn format_config_commander() {
        let config = FormatConfig::commander();
        assert_eq!(config.starting_life, 40);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 6);
        assert_eq!(config.deck_size, 100);
        assert!(config.singleton);
        assert!(config.command_zone);
        assert_eq!(config.commander_damage_threshold, Some(21));
        assert!(!config.team_based);
    }

    #[test]
    fn format_config_tiny_leaders() {
        let config = FormatConfig::tiny_leaders();
        assert_eq!(config.format, GameFormat::TinyLeaders);
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 2);
        assert_eq!(config.deck_size, 50);
        assert!(config.singleton);
        assert!(config.command_zone);
        assert_eq!(config.commander_damage_threshold, None);
        assert!(!config.uses_commander);
        assert!(!config.team_based);
    }

    #[test]
    fn format_config_premodern() {
        let config = FormatConfig::premodern();
        assert_eq!(config.format, GameFormat::Premodern);
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 2);
        assert_eq!(config.deck_size, 60);
        assert!(!config.singleton);
        assert!(!config.command_zone);
        assert_eq!(config.commander_damage_threshold, None);
        assert!(!config.uses_commander);
        assert!(!config.team_based);
    }

    #[test]
    fn format_config_free_for_all() {
        let config = FormatConfig::free_for_all();
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 6);
        assert_eq!(config.deck_size, 60);
        assert!(!config.singleton);
        assert!(!config.command_zone);
    }

    #[test]
    fn format_config_two_headed_giant() {
        let config = FormatConfig::two_headed_giant();
        assert_eq!(config.starting_life, 30);
        assert_eq!(config.min_players, 4);
        assert_eq!(config.max_players, 4);
        assert!(config.team_based);
        assert_eq!(
            config.topology(),
            FormatTopology::FixedTeams {
                team_size: 2,
                team_count: 2,
                turn_structure: TurnStructure::SharedTeamTurns,
            }
        );
        assert_eq!(config.starting_life_for_seat(), 15);
    }

    #[test]
    fn format_registry_includes_two_headed_giant() {
        let registry = GameFormat::registry();
        let metadata = registry
            .iter()
            .find(|metadata| metadata.format == GameFormat::TwoHeadedGiant)
            .expect("Two-Headed Giant should be user-selectable");

        assert_eq!(metadata.label, "Two-Headed Giant");
        assert_eq!(metadata.short_label, "2HG");
        assert_eq!(metadata.description, "4 players, two teams of two");
        assert_eq!(metadata.group, FormatGroup::Multiplayer);
        assert_eq!(metadata.default_config.min_players, 4);
        assert_eq!(metadata.default_config.max_players, 4);
        assert_eq!(metadata.default_config.starting_life, 30);
        assert!(metadata.default_config.team_based);
        assert!(!metadata.default_config.supplies_fixed_deck);
    }

    #[test]
    fn starting_life_for_seat_preserves_non_team_formats() {
        assert_eq!(FormatConfig::standard().starting_life_for_seat(), 20);
        assert_eq!(FormatConfig::commander().starting_life_for_seat(), 40);
    }

    #[test]
    fn sideboard_policy_matches_format_semantics() {
        assert_eq!(
            GameFormat::Standard.sideboard_policy(),
            SideboardPolicy::Limited(15)
        );
        assert_eq!(
            GameFormat::Pauper.sideboard_policy(),
            SideboardPolicy::Limited(15)
        );
        assert_eq!(
            GameFormat::Premodern.sideboard_policy(),
            SideboardPolicy::Limited(15)
        );
        assert_eq!(
            GameFormat::Commander.sideboard_policy(),
            SideboardPolicy::Forbidden
        );
        assert_eq!(
            GameFormat::Brawl.sideboard_policy(),
            SideboardPolicy::Forbidden
        );
        assert_eq!(
            GameFormat::HistoricBrawl.sideboard_policy(),
            SideboardPolicy::Forbidden
        );
        assert_eq!(
            GameFormat::TinyLeaders.sideboard_policy(),
            SideboardPolicy::Limited(10)
        );
        assert_eq!(
            GameFormat::FreeForAll.sideboard_policy(),
            SideboardPolicy::Unlimited
        );
        assert_eq!(
            GameFormat::TwoHeadedGiant.sideboard_policy(),
            SideboardPolicy::Unlimited
        );
    }

    #[test]
    fn sideboard_policy_serializes_as_tagged_union() {
        // Unit variants emit {"type": "..."} with no "data" field — the
        // frontend consumer must switch on `.type`, never destructure `.data`
        // unconditionally.
        let forbidden = serde_json::to_string(&SideboardPolicy::Forbidden).unwrap();
        assert_eq!(forbidden, r#"{"type":"Forbidden"}"#);

        let unlimited = serde_json::to_string(&SideboardPolicy::Unlimited).unwrap();
        assert_eq!(unlimited, r#"{"type":"Unlimited"}"#);

        // Tuple variant carries the cap in `data`.
        let limited = serde_json::to_string(&SideboardPolicy::Limited(15)).unwrap();
        assert_eq!(limited, r#"{"type":"Limited","data":15}"#);
    }

    #[test]
    fn deck_copy_limit_serializes_as_tagged_union() {
        // Unit variant emits {"type": "..."} with no "data" field; the frontend
        // must switch on `.type`, never destructure `.data` unconditionally.
        let unlimited = serde_json::to_string(&DeckCopyLimit::Unlimited).unwrap();
        assert_eq!(unlimited, r#"{"type":"Unlimited"}"#);

        // Tuple variant carries the cap in `data`.
        let up_to = serde_json::to_string(&DeckCopyLimit::UpTo(7)).unwrap();
        assert_eq!(up_to, r#"{"type":"UpTo","data":7}"#);

        // Round-trips both directions.
        let parsed: DeckCopyLimit = serde_json::from_str(r#"{"type":"Unlimited"}"#).unwrap();
        assert_eq!(parsed, DeckCopyLimit::Unlimited);
        let parsed: DeckCopyLimit = serde_json::from_str(r#"{"type":"UpTo","data":9}"#).unwrap();
        assert_eq!(parsed, DeckCopyLimit::UpTo(9));
    }

    #[test]
    fn format_config_oathbreaker() {
        let config = FormatConfig::oathbreaker();
        assert_eq!(config.format, GameFormat::Oathbreaker);
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 4);
        assert_eq!(config.deck_size, 60);
        assert!(config.singleton);
        assert!(config.command_zone);
        assert_eq!(config.commander_damage_threshold, None);
        assert!(!config.uses_commander);
        assert!(!config.team_based);
        assert_eq!(
            GameFormat::Oathbreaker.sideboard_policy(),
            SideboardPolicy::Forbidden
        );
        assert!(GameFormat::Oathbreaker.grants_free_first_mulligan());
        assert!(!GameFormat::Oathbreaker.uses_commander());
        assert_eq!(GameFormat::Oathbreaker.legality_format(), None);
    }

    #[test]
    fn format_config_serde_roundtrip() {
        let configs = vec![
            FormatConfig::standard(),
            FormatConfig::commander(),
            FormatConfig::pioneer(),
            FormatConfig::premodern(),
            FormatConfig::historic(),
            FormatConfig::pauper(),
            FormatConfig::tiny_leaders(),
            FormatConfig::oathbreaker(),
            FormatConfig::brawl(),
            FormatConfig::historic_brawl(),
            FormatConfig::free_for_all(),
            FormatConfig::two_headed_giant(),
            FormatConfig::archenemy(),
            FormatConfig::limited(),
        ];
        for config in configs {
            let json = serde_json::to_string(&config).unwrap();
            let deserialized: FormatConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(config, deserialized);
        }
    }

    #[test]
    fn format_config_limited() {
        let config = FormatConfig::limited();
        assert_eq!(config.format, GameFormat::Limited);
        assert_eq!(config.starting_life, 20);
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 2);
        assert_eq!(config.deck_size, 40);
        assert!(!config.singleton);
        assert!(!config.command_zone);
        assert_eq!(config.commander_damage_threshold, None);
        assert!(!config.team_based);
    }

    #[test]
    fn limited_legality_format_is_none() {
        assert_eq!(GameFormat::Limited.legality_format(), None);
    }

    #[test]
    fn limited_sideboard_policy_is_unlimited() {
        assert_eq!(
            GameFormat::Limited.sideboard_policy(),
            SideboardPolicy::Unlimited
        );
    }

    #[test]
    fn limited_no_free_first_mulligan() {
        assert!(!GameFormat::Limited.grants_free_first_mulligan());
    }

    #[test]
    fn premodern_uses_normal_constructed_mulligan() {
        assert!(!GameFormat::Modern.grants_free_first_mulligan());
        assert!(!GameFormat::Premodern.grants_free_first_mulligan());
        assert!(!GameFormat::Legacy.grants_free_first_mulligan());
    }

    #[test]
    fn premodern_legality_format() {
        assert_eq!(
            GameFormat::Premodern.legality_format(),
            Some(LegalityFormat::Premodern)
        );
    }

    #[test]
    fn limited_label() {
        assert_eq!(GameFormat::Limited.label(), "Limited");
    }

    #[test]
    fn limited_for_format_roundtrip() {
        assert_eq!(
            FormatConfig::for_format(GameFormat::Limited),
            FormatConfig::limited()
        );
    }

    #[test]
    fn premodern_for_format_roundtrip() {
        assert_eq!(
            FormatConfig::for_format(GameFormat::Premodern),
            FormatConfig::premodern()
        );
    }

    #[test]
    fn uses_commander_matches_default_config_and_threshold() {
        // The `GameFormat::uses_commander()` predicate, the derived
        // `FormatConfig::uses_commander` field, and the existence of a
        // commander-damage threshold must all agree for every variant.
        for meta in GameFormat::registry() {
            let expected = meta.format.uses_commander();
            assert_eq!(
                meta.default_config.uses_commander, expected,
                "{:?}: registry default disagrees with predicate",
                meta.format
            );
            assert_eq!(
                meta.default_config.commander_damage_threshold.is_some(),
                expected,
                "{:?}: commander_damage_threshold presence must match uses_commander",
                meta.format
            );
            // The derived `supplies_fixed_deck` field must agree with the
            // predicate for every variant (engine is the single authority for
            // which formats auto-supply their deck).
            assert_eq!(
                meta.default_config.supplies_fixed_deck,
                meta.format.supplies_fixed_deck(),
                "{:?}: registry default disagrees with supplies_fixed_deck predicate",
                meta.format
            );
        }
        // Variants not in the user-facing registry still respect the invariant.
        for format in [GameFormat::TwoHeadedGiant, GameFormat::Limited] {
            let config = FormatConfig::for_format(format);
            assert_eq!(config.uses_commander, format.uses_commander());
            assert_eq!(config.supplies_fixed_deck, format.supplies_fixed_deck());
        }
    }

    #[test]
    fn limited_in_registry() {
        let registry = GameFormat::registry();
        let entry = registry
            .iter()
            .find(|m| m.format == GameFormat::Limited)
            .expect("Limited must be in registry");
        assert_eq!(entry.group, FormatGroup::Limited);
        assert_eq!(entry.short_label, "LIM");
    }

    #[test]
    fn archenemy_registry_entry_uses_default_topology() {
        let registry = GameFormat::registry();
        let entry = registry
            .iter()
            .find(|m| m.format == GameFormat::Archenemy)
            .expect("Archenemy must be in registry");
        assert_eq!(entry.group, FormatGroup::Multiplayer);
        assert_eq!(entry.short_label, "ARC");
        assert_eq!(entry.default_config, FormatConfig::archenemy());
        assert_eq!(entry.default_config.min_players, 2);
        assert_eq!(entry.default_config.max_players, 6);
        assert_eq!(entry.default_config.deck_size, 60);
        assert!(entry.default_config.command_zone);
        assert!(!entry.default_config.team_based);
        assert_eq!(entry.default_config.commander_damage_threshold, None);
        assert_eq!(entry.default_config.archenemy_player(), Some(PlayerId(0)));
    }

    #[test]
    fn format_config_horde() {
        let config = FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset());
        assert_eq!(config.format, GameFormat::Horde);
        // `starting_life` is now only the fallback base; the authoritative
        // survivor combined base lives on the ruleset (community rules: 100).
        assert_eq!(config.starting_life, 100);
        assert_eq!(
            config.horde_ruleset.as_ref().unwrap().combined_base_life,
            100
        );
        assert_eq!(config.min_players, 2);
        assert_eq!(config.max_players, 5);
        assert!(config.command_zone);
        assert!(!config.team_based);
        assert!(!config.uses_commander);
        // Survivors build their own decks; only the Horde seat is engine-supplied.
        assert!(!config.supplies_fixed_deck);
        assert_eq!(config.archenemy_player(), Some(PlayerId(0)));
        let ruleset = config
            .horde_ruleset
            .as_ref()
            .expect("horde config has ruleset");
        assert_eq!(ruleset.challenge_deck, ChallengeDeck::CybermanHorde);
        assert_eq!(ruleset.survivor_setup_turns, 3);
        assert!(ruleset.horde_creatures_forced_attackers);
    }

    #[test]
    fn horde_uses_one_vs_many_shared_turn_topology() {
        let config = FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset());
        assert_eq!(
            config.topology(),
            FormatTopology::OneVsMany {
                archenemy: PlayerId(0),
                turn_structure: TurnStructure::SharedTeamTurns,
            }
        );
    }

    #[test]
    fn horde_survivors_take_the_first_turn_not_the_horde() {
        // Regression guard for the review's B1 blocker: because Horde reuses the
        // Archenemy OneVsMany topology, the naive path would hand the Horde
        // (the archenemy seat) the first turn. Survivors must go first.
        let config = FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset());
        let horde_seat = config.archenemy_player().expect("horde seat");
        let first = config.starting_player();
        assert_ne!(first, horde_seat, "the Horde must not take the first turn");
        assert_eq!(first, PlayerId(1));
    }

    #[test]
    fn horde_seat_is_not_given_archenemy_forty_life() {
        // B1 blocker guard: the Archenemy OneVsMany arm grants 40 life to the
        // special seat. The Horde has no life total and must not inherit 40.
        let ruleset = ChallengeDeck::CybermanHorde.default_ruleset();
        let base = ruleset.combined_base_life;
        let config = FormatConfig::horde(ruleset);
        let horde_seat = config.archenemy_player().expect("horde seat");
        // Load-bearing: NOT the archenemy's 40. Every seat is pre-seeded with the
        // combined base (redistributed across survivors in `GameState::new`);
        // derive it from the ruleset rather than hardcoding, so this stays valid
        // as deck balance changes.
        assert_ne!(config.starting_life_for_player(horde_seat), 40);
        assert_eq!(config.starting_life_for_player(horde_seat), base);
        assert_eq!(config.starting_life_for_player(PlayerId(1)), base);
    }

    #[test]
    fn horde_registry_entry_uses_default_config() {
        let registry = GameFormat::registry();
        let entry = registry
            .iter()
            .find(|m| m.format == GameFormat::Horde)
            .expect("Horde must be in registry");
        assert_eq!(entry.group, FormatGroup::Multiplayer);
        assert_eq!(entry.short_label, "HRD");
        assert_eq!(
            entry.default_config,
            FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset())
        );
    }

    #[test]
    fn horde_ruleset_survives_config_serde_round_trip() {
        let config = FormatConfig::horde(ChallengeDeck::CybermanHorde.default_ruleset());
        let json = serde_json::to_string(&config).expect("serialize");
        let restored: FormatConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, restored);
    }

    #[test]
    fn non_horde_configs_have_no_horde_ruleset() {
        for meta in GameFormat::registry() {
            let has_ruleset = meta.default_config.horde_ruleset.is_some();
            assert_eq!(
                has_ruleset,
                meta.format == GameFormat::Horde,
                "{:?}: horde_ruleset presence must match the Horde format",
                meta.format
            );
        }
    }

    #[test]
    fn premodern_registry_entry_is_ordered_with_constructed_formats() {
        let registry = GameFormat::registry();
        let modern_index = registry
            .iter()
            .position(|m| m.format == GameFormat::Modern)
            .expect("Modern must be in registry");
        let premodern_index = registry
            .iter()
            .position(|m| m.format == GameFormat::Premodern)
            .expect("Premodern must be in registry");
        let legacy_index = registry
            .iter()
            .position(|m| m.format == GameFormat::Legacy)
            .expect("Legacy must be in registry");

        assert_eq!(premodern_index, modern_index + 1);
        assert_eq!(legacy_index, premodern_index + 1);
        assert_eq!(registry[premodern_index].short_label, "PRE");
        assert_eq!(registry[premodern_index].group, FormatGroup::Constructed);
    }

    #[test]
    fn registry_constructed_formats_have_legality_mapping() {
        for meta in GameFormat::registry()
            .into_iter()
            .filter(|meta| meta.group == FormatGroup::Constructed)
        {
            assert!(
                meta.format.legality_format().is_some(),
                "{:?} is constructed but has no legality mapping",
                meta.format
            );
        }
    }

    /// The Horde deck picker renders `ChallengeDeck::registry()` directly, so a
    /// deck missing from `ALL` would be silently unselectable. `metadata` is an
    /// exhaustive match (the compiler catches a missing arm); this catches a
    /// variant that was described but never added to `ALL`.
    #[test]
    fn registry_covers_every_challenge_deck() {
        let registry = ChallengeDeck::registry();
        assert_eq!(registry.len(), ChallengeDeck::ALL.len());

        for deck in ChallengeDeck::ALL {
            // Exhaustive match: adding a variant fails to compile here until it
            // is also appended to `ALL`.
            match deck {
                ChallengeDeck::CybermanHorde
                | ChallengeDeck::DndHorde
                | ChallengeDeck::ZombiesHorde
                | ChallengeDeck::SliversHorde
                | ChallengeDeck::HumansGodzillaHorde => {}
            }
            assert!(
                registry.iter().any(|meta| meta.deck == *deck),
                "{deck:?} is missing from the challenge-deck registry"
            );
        }
    }

    /// Every entry must be renderable (non-empty labels) and internally
    /// consistent — the bundled ruleset must describe the deck it is attached to,
    /// so a client can build a `FormatConfig` straight from a picked entry.
    #[test]
    fn registry_entries_are_renderable_and_self_consistent() {
        for meta in ChallengeDeck::registry() {
            assert!(!meta.label.is_empty(), "{:?} has no label", meta.deck);
            assert!(
                !meta.short_label.is_empty(),
                "{:?} has no short label",
                meta.deck
            );
            assert!(
                !meta.description.is_empty(),
                "{:?} has no description",
                meta.deck
            );
            assert_eq!(
                meta.default_ruleset.challenge_deck, meta.deck,
                "{:?} bundles a ruleset for a different deck",
                meta.deck
            );
            // Every entry ships a rendered rules summary the picker can display,
            // and it must be exactly what the deck's ruleset renders — the two
            // must not drift.
            assert_eq!(
                meta.rules,
                meta.default_ruleset.summary(),
                "{:?}'s bundled rules summary does not match its ruleset",
                meta.deck
            );
            assert!(
                !meta.rules.is_empty(),
                "{:?} has no rules summary",
                meta.deck
            );
            for line in &meta.rules {
                assert!(
                    !line.label.is_empty(),
                    "{:?} has an unlabeled rule",
                    meta.deck
                );
                assert!(
                    !line.detail.is_empty(),
                    "{:?}'s '{}' rule has no detail",
                    meta.deck,
                    line.label
                );
            }
        }
    }

    /// `HordeRuleset::summary` is the single authority for rules prose, so test it
    /// as a building block across the full `WaveTermination` space — including the
    /// `FixedCount` / `Fixed(n>1)` shapes no shipped deck uses yet (Theros decks
    /// will) — not just the current decks' output.
    #[test]
    fn ruleset_summary_renders_every_wave_shape() {
        let with_wave = |wave: WaveTermination| HordeRuleset {
            challenge_deck: ChallengeDeck::CybermanHorde,
            wave,
            survivor_setup_turns: 3,
            combined_base_life: 100,
            per_extra_survivor_life_delta: -15,
            horde_creatures_forced_attackers: true,
            legendary_death: HordeLegendaryDeath::Normal,
            post_combat_activation: HordePostCombatActivation::None,
        };
        // Pull the "Waves" line out of the rendered summary.
        let wave_detail = |wave: WaveTermination| -> String {
            with_wave(wave)
                .summary()
                .into_iter()
                .find(|l| l.label == "Waves")
                .expect("summary always has a Waves line")
                .detail
        };

        assert_eq!(
            wave_detail(WaveTermination::UntilNonToken {
                count: WaveCount::Fixed(1)
            }),
            "Reveals until the first nontoken card is cast"
        );
        assert_eq!(
            wave_detail(WaveTermination::UntilNonToken {
                count: WaveCount::Fixed(2)
            }),
            "Reveals until 2 nontoken cards are cast"
        );
        assert_eq!(
            wave_detail(WaveTermination::UntilNonToken {
                count: WaveCount::Snaking { min: 1, max: 3 }
            }),
            "Reveals until N nontokens are cast \u{2014} N snakes 1 \u{2192} 3 \u{2192} 1 each turn"
        );
        assert_eq!(
            wave_detail(WaveTermination::UntilRarityAtLeast(Rarity::Uncommon)),
            "A wave ends at the first uncommon or better card"
        );
        assert_eq!(
            wave_detail(WaveTermination::FixedCount(2)),
            "Reveals 2 cards each Horde turn"
        );
        // Singular grammar for a one-card fixed wave.
        assert_eq!(
            wave_detail(WaveTermination::FixedCount(1)),
            "Reveals 1 card each Horde turn"
        );
    }

    /// The life line must fold both the shared base and the per-extra-survivor
    /// scaling; a flat total (Theros: `delta == 0`) drops the scaling clause.
    #[test]
    fn ruleset_summary_life_line_handles_flat_and_scaled_totals() {
        let line = |base: i32, delta: i32| -> String {
            HordeRuleset {
                challenge_deck: ChallengeDeck::CybermanHorde,
                wave: WaveTermination::FixedCount(1),
                survivor_setup_turns: 3,
                combined_base_life: base,
                per_extra_survivor_life_delta: delta,
                horde_creatures_forced_attackers: true,
                legendary_death: HordeLegendaryDeath::Normal,
                post_combat_activation: HordePostCombatActivation::None,
            }
            .summary()
            .into_iter()
            .find(|l| l.label == "Survivor life")
            .expect("summary always has a Survivor life line")
            .detail
        };
        assert_eq!(line(100, -15), "100 life shared, -15 per extra survivor");
        assert_eq!(line(20, 0), "20 life, shared by all survivors");
    }

    /// The legendary axis is advanced-only: a `Normal` ruleset renders no
    /// "Legendary deaths" line (basic decks stay their four lines), while an
    /// `EtbThenPhaseOut` ruleset adds exactly one. This is the axis's contract
    /// with the picker — it surfaces only where the rule actually differs.
    #[test]
    fn ruleset_summary_shows_legendary_line_only_for_advanced_rule() {
        let with_legendary = |rule: HordeLegendaryDeath| HordeRuleset {
            challenge_deck: ChallengeDeck::CybermanHorde,
            wave: WaveTermination::FixedCount(1),
            survivor_setup_turns: 3,
            combined_base_life: 100,
            per_extra_survivor_life_delta: -15,
            horde_creatures_forced_attackers: true,
            legendary_death: rule,
            post_combat_activation: HordePostCombatActivation::None,
        };

        let basic = with_legendary(HordeLegendaryDeath::Normal).summary();
        assert!(
            !basic.iter().any(|l| l.label == "Legendary deaths"),
            "a Normal ruleset must not advertise the advanced legendary rule"
        );
        assert_eq!(basic.len(), 4, "basic decks stay their four rule lines");

        let advanced = with_legendary(HordeLegendaryDeath::EtbThenPhaseOut).summary();
        let legendary_line = advanced
            .iter()
            .find(|l| l.label == "Legendary deaths")
            .expect("an EtbThenPhaseOut ruleset must surface a Legendary deaths line");
        assert!(
            legendary_line.detail.contains("phases out"),
            "the legendary line must describe the phase-out behavior"
        );
    }

    /// The post-combat-activation axis is advanced-only too: `None` renders no
    /// "Post-combat" line; `OncePerPermanent` adds exactly one.
    #[test]
    fn ruleset_summary_shows_post_combat_line_only_for_advanced_rule() {
        let with_activation = |rule: HordePostCombatActivation| HordeRuleset {
            challenge_deck: ChallengeDeck::CybermanHorde,
            wave: WaveTermination::FixedCount(1),
            survivor_setup_turns: 3,
            combined_base_life: 100,
            per_extra_survivor_life_delta: -15,
            horde_creatures_forced_attackers: true,
            legendary_death: HordeLegendaryDeath::Normal,
            post_combat_activation: rule,
        };

        let basic = with_activation(HordePostCombatActivation::None).summary();
        assert!(
            !basic.iter().any(|l| l.label == "Post-combat"),
            "a None ruleset must not advertise post-combat activation"
        );
        assert_eq!(basic.len(), 4, "basic decks stay their four rule lines");

        let advanced = with_activation(HordePostCombatActivation::OncePerPermanent).summary();
        let line = advanced
            .iter()
            .find(|l| l.label == "Post-combat")
            .expect("an OncePerPermanent ruleset must surface a Post-combat line");
        assert!(
            line.detail.contains("infinite mana"),
            "the post-combat line must mention the Horde's infinite mana"
        );
    }

    /// The whole point of the summary is to show how decks DIFFER — two decks with
    /// different wave rules must produce different "Waves" lines.
    #[test]
    fn different_decks_produce_different_wave_summaries() {
        let waves = |deck: ChallengeDeck| -> String {
            deck.metadata()
                .rules
                .into_iter()
                .find(|l| l.label == "Waves")
                .unwrap()
                .detail
        };
        assert_ne!(
            waves(ChallengeDeck::CybermanHorde),
            waves(ChallengeDeck::ZombiesHorde)
        );
        assert_ne!(
            waves(ChallengeDeck::CybermanHorde),
            waves(ChallengeDeck::DndHorde)
        );
    }

    /// Pin each deck's defining wave rule — these are what make them play
    /// differently, and a silent swap would change the whole feel of a deck.
    #[test]
    fn challenge_decks_keep_their_defining_wave_rules() {
        assert_eq!(
            ChallengeDeck::CybermanHorde.default_ruleset().wave,
            WaveTermination::UntilNonToken {
                count: WaveCount::Fixed(1)
            }
        );
        assert_eq!(
            ChallengeDeck::DndHorde.default_ruleset().wave,
            WaveTermination::UntilRarityAtLeast(Rarity::Uncommon)
        );
        assert_eq!(
            ChallengeDeck::ZombiesHorde.default_ruleset().wave,
            WaveTermination::UntilNonToken {
                count: WaveCount::Snaking { min: 1, max: 3 }
            }
        );
    }

    /// Pin the published community survivor-life rule (hordemagic basic rules:
    /// "combined, shared and start at 100 life. Each player reduces {15} life ...
    /// beyond the 1st"). The `horde_multiplayer` mechanism tests are deliberately
    /// balance-agnostic (they derive from the ruleset), so this is where the
    /// concrete 100 / −15 → 100·85·70 numbers are actually asserted; a wrong base
    /// or delta slips past those tests but not this one.
    #[test]
    fn community_decks_use_the_published_shared_life_rule() {
        for deck in ChallengeDeck::ALL {
            let r = deck.default_ruleset();
            assert_eq!(
                r.combined_base_life, 100,
                "{deck:?} must use the 100-life community base"
            );
            assert_eq!(
                r.per_extra_survivor_life_delta, -15,
                "{deck:?} must reduce 15 per extra survivor"
            );
            // 1 / 2 / 3 survivors → 100 / 85 / 70.
            let combined =
                |n: i32| r.combined_base_life + r.per_extra_survivor_life_delta * (n - 1);
            assert_eq!((combined(1), combined(2), combined(3)), (100, 85, 70));
        }
    }

    /// A constant schedule is the classic rule and must not vary by turn.
    #[test]
    fn fixed_wave_count_is_constant_across_turns() {
        let fixed = WaveCount::Fixed(1);
        for turn in 0..10 {
            assert_eq!(fixed.nontokens_for_turn(turn), 1);
        }
    }

    /// The Zombies Horde's published ramp: "Wave 1 ... then Wave 2 ... and so on
    /// for Wave 3. On its next turn, it will descend back down to Wave 2, snaking
    /// back and forth." Pin the exact sequence over two full periods.
    #[test]
    fn snaking_wave_count_ramps_up_and_back_down() {
        let snake = WaveCount::Snaking { min: 1, max: 3 };
        let seq: Vec<u32> = (0..9).map(|t| snake.nontokens_for_turn(t)).collect();
        assert_eq!(seq, vec![1, 2, 3, 2, 1, 2, 3, 2, 1]);
    }

    /// The endpoints must not repeat — a naive "up then reversed" schedule would
    /// emit 1,2,3,3,2,1 and linger a turn too long at each extreme.
    #[test]
    fn snaking_wave_count_does_not_repeat_its_endpoints() {
        let snake = WaveCount::Snaking { min: 2, max: 4 };
        let seq: Vec<u32> = (0..8).map(|t| snake.nontokens_for_turn(t)).collect();
        assert_eq!(seq, vec![2, 3, 4, 3, 2, 3, 4, 3]);
    }

    /// A degenerate range must still yield a terminating wave rather than
    /// dividing by zero on the period.
    #[test]
    fn snaking_wave_count_handles_degenerate_ranges() {
        assert_eq!(
            WaveCount::Snaking { min: 2, max: 2 }.nontokens_for_turn(7),
            2
        );
        assert_eq!(
            WaveCount::Snaking { min: 3, max: 1 }.nontokens_for_turn(7),
            3
        );
    }
}
