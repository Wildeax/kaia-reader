//! Settings keyboards use the selected app language.
use kaia_ui::controls::{KeyAction, KeyDef, KeyLabels, KeyboardLayout, ENGLISH};

const fn letter(c: char) -> KeyDef {
    KeyDef {
        action: KeyAction::Char(c),
        width: 1.0,
    }
}
static SPANISH: KeyboardLayout = KeyboardLayout {
    name: "es",
    labels: KeyLabels {
        shift: "Mayús",
        caps: "Bloq",
        delete: "Borrar",
        enter: "Intro",
        space: "Espacio",
        hide: "Ocultar",
    },
    letters: &[
        ENGLISH.letters[0],
        &[
            letter('a'),
            letter('s'),
            letter('d'),
            letter('f'),
            letter('g'),
            letter('h'),
            letter('j'),
            letter('k'),
            letter('l'),
            letter('ñ'),
        ],
        ENGLISH.letters[2],
        ENGLISH.letters[3],
    ],
    numbers: ENGLISH.numbers,
    symbols: &[
        ENGLISH.symbols[0],
        &[
            letter('^'),
            letter('_'),
            letter('#'),
            letter('á'),
            letter('é'),
            letter('í'),
            letter('ó'),
            letter('ú'),
            letter('ü'),
            letter('¿'),
        ],
        &[
            ENGLISH.symbols[2][0],
            letter('¡'),
            letter('&'),
            letter('*'),
            letter('+'),
            letter('='),
            letter('('),
            letter(')'),
            ENGLISH.symbols[2][8],
        ],
        ENGLISH.symbols[3],
    ],
};
pub fn layout(language: &str) -> &'static KeyboardLayout {
    if language.split(['-', '_']).next() == Some("es") {
        &SPANISH
    } else {
        &ENGLISH
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spanish_has_letters_accents_and_localized_actions() {
        let layout = layout("es-ES");
        let characters = |layout: &KeyboardLayout| {
            [layout.letters, layout.numbers, layout.symbols]
                .into_iter()
                .flat_map(|rows| rows.iter().flat_map(|row| row.iter()))
                .filter_map(|key| {
                    if let KeyAction::Char(c) = key.action {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert!(characters(&ENGLISH).is_subset(&characters(layout)));
        assert_eq!(layout.name, "es");
        assert_eq!(layout.labels.enter, "Intro");
        assert!(layout
            .letters
            .iter()
            .flat_map(|row| row.iter())
            .any(|key| key.action == KeyAction::Char('ñ')));
        assert!(layout
            .symbols
            .iter()
            .flat_map(|row| row.iter())
            .any(|key| key.action == KeyAction::Char('á')));
    }
}
