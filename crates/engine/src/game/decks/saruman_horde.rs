//! Middle-earth "Saruman Horde" — the concrete decklist for
//! [`crate::types::format::ChallengeDeck::SarumanHorde`].
//!
//! Data, not logic. The Horde seat's engine-supplied library is 280 cards:
//! 215 real Universes Beyond (LTR / LTC / The Hobbit) non-token cards resolved
//! from the card DB by name, plus 65 predefined Orc Army tokens materialized
//! from `game::token_presets::known_token_body_by_name`. Tokens can never be
//! cast (CR 111), so they wait in the library and enter the battlefield when
//! revealed (see `game::horde::reveal_library_token`); non-tokens are free-cast
//! on reveal.
//!
//! Transcribed from the hordemagic.com "Saruman, the White Hand" Horde list.
//! The published list is faithfully reproduced here; its own "299" headline
//! count is inaccurate — the actual 60 distinct entries sum to 215 non-tokens
//! plus 65 Orc Army tokens = 280.

/// The non-token cards, as `(count, exact card name)` pairs. Each name resolves
/// against the runtime card database (`CardDatabase::get_face_by_name`); the
/// injection in `deck_loading::load_horde_library` panics on any name that fails
/// to resolve so a transcription error surfaces immediately rather than shipping
/// a silently-truncated library.
///
/// 29 legendary creatures + 60 creatures + 84 instants/sorceries + 16 artifacts
/// + 25 enchantments + 1 commander = 215 non-token cards.
///
/// NOTE: the four The Hobbit (`hob`) cards below — Misty Mountains Raider, Great
/// Ugly-Looking Goblin, Crude Bent Blade, Goblin Plate Mail — are real cards
/// (verified on Scryfall) whose exact spellings are used here, but they are NOT
/// yet present in the July-2024 `mtgish-cards.json` snapshot and will fail to
/// resolve (panic) until the card DB is regenerated to include The Hobbit.
pub const SARUMAN_HORDE_NONTOKEN_CARDS: &[(u32, &str)] = &[
    // Commander (1).
    (1, "Saruman, the White Hand"),
    // Legendary creatures (29).
    (8, "Uglúk of the White Hand"),
    (1, "The Balrog, Durin's Bane"),
    (1, "Saruman of Many Colors"),
    (2, "Saruman the White"),
    (2, "Gothmog, Morgul Lieutenant"),
    (5, "Gríma Wormtongue"),
    (1, "Gríma, Saruman's Footman"),
    (1, "Shagrat, Loot Bearer"),
    (7, "The Mouth of Sauron"),
    (1, "Fires of Mount Doom"),
    // Creatures (60).
    (23, "Goblin Assailant"),
    (1, "Goblin Cratermaker"),
    (3, "Goblin Dark-Dwellers"),
    (3, "Guttersnipe"),
    (8, "Mirkwood Bats"),
    (4, "Misty Mountains Raider"), // The Hobbit (hob) — not in current card DB.
    (3, "Great Ugly-Looking Goblin"), // The Hobbit (hob) — not in current card DB.
    (10, "Uruk-hai Berserker"),
    (5, "Willow-Wind"),
    // Instants & sorceries (84).
    (2, "Assault on Osgiliath"),
    (5, "Bitter Downfall"),
    (1, "Blasphemous Act"),
    (6, "Cast into the Fire"),
    (1, "Dreadful as the Storm"),
    (1, "Extract from Darkness"),
    (3, "Feed the Swarm"),
    (1, "Fear, Fire, Foes!"),
    (6, "Fire of Orthanc"),
    (3, "Flame of Anor"),
    (7, "Foray of Orcs"),
    (6, "Isolation at Orthanc"),
    (1, "Languish"),
    (9, "Lash of the Balrog"),
    (1, "Lidless Gaze"),
    (1, "Orcish Medicine"),
    (1, "Reanimate"),
    (14, "Rise of the Witch-king"),
    (1, "Shadow of the Enemy"),
    (2, "Smite the Deathless"),
    (1, "Subjugate the Hobbits"),
    (3, "Surrounded by Orcs"),
    (1, "Taunt from the Rampart"),
    (1, "Too Greedily, Too Deep"),
    (5, "Treason of Isengard"),
    (1, "Wake the Dragon"),
    // Artifacts (16).
    (3, "Barrow-Blade"),
    (9, "Crude Bent Blade"),  // The Hobbit (hob) — not in current card DB.
    (4, "Goblin Plate Mail"), // The Hobbit (hob) — not in current card DB.
    // Enchantments (25).
    (7, "Fiery Inscription"),
    (2, "Call of the Ring"),
    (3, "Book of Mazarbul"),
    (1, "Fall of Cair Andros"),
    (1, "In the Darkness Bind Them"),
    (3, "Leyline of Punishment"),
    (1, "March from the Black Gate"),
    (3, "Morgul-Knife Wound"),
    (1, "One Ring to Rule Them All"),
    (2, "Storm of Saruman"),
    (1, "The Bath Song"),
];

/// The predefined tokens, as `(count, preset token name)` pairs. Each name is
/// resolved to a `TokenCharacteristics` body via
/// `token_presets::known_token_body_by_name`; the body is materialized into a
/// library-resident token object (see `deck_loading::create_horde_library_token`).
///
/// 65 Orc Army tokens (the 0/0 black Amass Orc Army body).
pub const SARUMAN_HORDE_TOKENS: &[(u32, &str)] = &[(65, "Orc Army")];

/// Total non-token card count (for coverage assertions / library-size sanity).
pub fn nontoken_card_count() -> u32 {
    SARUMAN_HORDE_NONTOKEN_CARDS
        .iter()
        .map(|(count, _)| *count)
        .sum()
}

/// Total token count.
pub fn token_count() -> u32 {
    SARUMAN_HORDE_TOKENS.iter().map(|(count, _)| *count).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decklist_totals_are_280() {
        assert_eq!(nontoken_card_count(), 215, "215 non-token cards");
        assert_eq!(token_count(), 65, "65 tokens");
        assert_eq!(nontoken_card_count() + token_count(), 280);
    }

    /// Every token preset name must resolve to a single unambiguous body — the
    /// injection depends on `known_token_body_by_name` returning `Some`.
    #[test]
    fn token_preset_names_resolve() {
        for (_, name) in SARUMAN_HORDE_TOKENS {
            assert!(
                crate::game::token_presets::known_token_body_by_name(name).is_some(),
                "token preset '{name}' must resolve to a unique body"
            );
        }
    }
}
