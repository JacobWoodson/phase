//! Concrete challenge-deck decklists for the Horde format.
//!
//! Each module is pure DATA — the (count, name) card lists and token bodies that
//! make up one [`crate::types::format::ChallengeDeck`]. The generic Horde spine
//! (`game::horde`) and the seat-scoped injection (`game::deck_loading`) consume
//! these; a new deck is a peer module here plus one `ChallengeDeck` variant.
pub mod cyberman_horde;
pub mod dnd_horde;
pub mod humans_godzilla_horde;
// LOTR "Two Towers" two-Horde decks (Sauron + Saruman). Verified card lists;
// not yet wired to `ChallengeDeck` variants — that wiring needs the uncommon+
// wave's per-card rarity, the Amass Orc Army grow mechanic, and (for Saruman)
// a card DB regenerated to include The Hobbit set. Tracked as follow-ups.
pub mod saruman_horde;
pub mod sauron_horde;
pub mod slivers_horde;
pub mod zombies_horde;
