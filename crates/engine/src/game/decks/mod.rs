//! Concrete challenge-deck decklists for the Horde format.
//!
//! Each module is pure DATA — the (count, name) card lists and token bodies that
//! make up one [`crate::types::format::ChallengeDeck`]. The generic Horde spine
//! (`game::horde`) and the seat-scoped injection (`game::deck_loading`) consume
//! these; a new deck is a peer module here plus one `ChallengeDeck` variant.
pub mod cyberman_horde;
