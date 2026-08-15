//! The Lord of the Rings "Sauron, the Dark Lord Horde" — the concrete decklist
//! for the `ChallengeDeck::SauronHorde` Horde seat.
//!
//! Data, not logic. The Horde seat's engine-supplied library is ~300 cards:
//! real Universes Beyond (LTR / LTC) non-token cards resolved from the card DB
//! by name, plus predefined Orc Army / Wraith tokens materialized from
//! `game::token_presets::known_token_body_by_name`. Tokens can never be cast
//! (CR 111), so they wait in the library and enter the battlefield when revealed
//! (see `game::horde::reveal_library_token`); non-tokens are free-cast on reveal.

/// The non-token cards, as `(count, exact card name)` pairs. Each name resolves
/// against the runtime card database (`CardDatabase::get_face_by_name`); the
/// injection in `deck_loading::load_horde_library` panics on any name that fails
/// to resolve so a transcription error surfaces immediately rather than shipping
/// a silently-truncated library.
///
/// 170 creatures (incl. the legendary "boss" cards + commander) + 23 spells +
/// 1 artifact + 4 enchantments = 198 non-token cards.
pub const SAURON_HORDE_NONTOKEN_CARDS: &[(u32, &str)] = &[
    // Commander + legendary "boss" cards (18).
    (1, "Sauron, the Dark Lord"),
    (1, "Sauron, Lord of the Rings"),
    (1, "Sauron, the Lidless Eye"),
    (1, "Sauron, the Necromancer"),
    (1, "Witch-king of Angmar"),
    (1, "Witch-king, Bringer of Ruin"),
    (1, "Grond, the Gatebreaker"),
    (1, "Lord of the Nazgûl"),
    (1, "The One Ring"),
    (1, "Shelob, Child of Ungoliant"),
    (4, "Gothmog, Morgul Lieutenant"),
    (4, "Mauhúr, Uruk-hai Captain"),
    // Creatures (152).
    (18, "Warg Rider"),
    (11, "Corsairs of Umbar"),
    (3, "Gruesome Scourger"),
    (11, "Dunland Crebain"),
    (3, "Easterling Vanguard"),
    (1, "Goblin Cratermaker"),
    (3, "Grishnákh, Brash Instigator"),
    (1, "Hostage Taker"),
    (1, "Inferno Titan"),
    (11, "Merciless Executioner"),
    (6, "Mordor Trebuchet"),
    (5, "Moria Marauder"),
    (9, "Nazgûl"),
    (8, "Oliphaunt"),
    (4, "Olog-hai Crusher"),
    (14, "Orcish Siegemaster"),
    (1, "Rampaging War Mammoth"),
    (3, "Ringwraiths"),
    (1, "Scourge of the Throne"),
    (6, "Troll of Khazad-dûm"),
    (10, "Uruk-hai Berserker"),
    (10, "Voracious Fell Beast"),
    (12, "Warbeast of Gorgoroth"),
    // Instants / sorceries (23).
    (1, "Living Death"),
    (1, "Decree of Pain"),
    (1, "Horses of the Bruinen"),
    (6, "Sam's Desperate Rescue"),
    (4, "The Black Breath"),
    (4, "Claim the Precious"),
    (2, "Breaking of the Fellowship"),
    (4, "Ranger's Firebrand"),
    // Artifacts (1).
    (1, "Thorn of Amethyst"),
    // Enchantments (4).
    (3, "One Ring to Rule Them All"),
    (1, "In the Darkness Bind Them"),
];

/// The predefined tokens, as `(count, preset token name)` pairs. Each name is
/// resolved to a `TokenCharacteristics` body via
/// `token_presets::known_token_body_by_name`; the body is materialized into a
/// library-resident token object (see `deck_loading::create_horde_library_token`).
///
/// 77 Orc Army + 28 Wraith = 105 tokens.
pub const SAURON_HORDE_TOKENS: &[(u32, &str)] = &[(77, "Orc Army"), (28, "Wraith")];

/// Total non-token card count (for coverage assertions / library-size sanity).
pub fn nontoken_card_count() -> u32 {
    SAURON_HORDE_NONTOKEN_CARDS
        .iter()
        .map(|(count, _)| *count)
        .sum()
}

/// Total token count.
pub fn token_count() -> u32 {
    SAURON_HORDE_TOKENS.iter().map(|(count, _)| *count).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decklist_totals_are_303() {
        assert_eq!(nontoken_card_count(), 198, "198 non-token cards");
        assert_eq!(token_count(), 105, "105 tokens");
        assert_eq!(nontoken_card_count() + token_count(), 303);
    }

    /// Every token preset name must resolve to a single unambiguous body — the
    /// injection depends on `known_token_body_by_name` returning `Some`.
    #[test]
    fn token_preset_names_resolve() {
        for (_, name) in SAURON_HORDE_TOKENS {
            assert!(
                crate::game::token_presets::known_token_body_by_name(name).is_some(),
                "token preset '{name}' must resolve to a unique body"
            );
        }
    }
}
