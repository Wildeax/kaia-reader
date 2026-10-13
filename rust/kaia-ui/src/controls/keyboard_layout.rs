//! Key tables for the on-screen keyboard. A layout is data: three layers of
//! rows, each row a list of keys, plus the words the keys show. No row is
//! wider than ten units, so every layer fits the same grid. To add
//! a language, add a table like [`ENGLISH`] and pass it to
//! [`OnScreenKeyboard::new`](super::OnScreenKeyboard::new).

/// A layer of keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Letters, with shift and caps.
    Letters,
    /// Digits and common math signs.
    Numbers,
    /// Punctuation and brackets.
    Symbols,
}

/// What a key does when tapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Types the character. Letters follow the shift state.
    Char(char),
    /// Cycles lower case, shift (next letter only) and caps lock.
    Shift,
    /// Deletes the character before the caret.
    Backspace,
    /// Submits the field (`Key::Press`).
    Enter,
    /// Types a space.
    Space,
    /// Switches to another layer.
    Layer(Layer),
    /// Asks the app to hide the keyboard. Emits the `on_hide` event.
    Hide,
}

/// One key in a row. `width` is in units: ten units fill the grid, and a
/// shorter row is centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyDef {
    pub action: KeyAction,
    pub width: f32,
}

/// The words on keys that are not characters. Each language provides its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyLabels {
    pub shift: &'static str,
    pub caps: &'static str,
    pub delete: &'static str,
    pub enter: &'static str,
    pub space: &'static str,
    pub hide: &'static str,
}

/// The keys of one language, on all three layers.
#[derive(Clone, Copy, Debug)]
pub struct KeyboardLayout {
    pub name: &'static str,
    pub labels: KeyLabels,
    pub letters: &'static [&'static [KeyDef]],
    pub numbers: &'static [&'static [KeyDef]],
    pub symbols: &'static [&'static [KeyDef]],
}

const fn ch(c: char) -> KeyDef {
    KeyDef {
        action: KeyAction::Char(c),
        width: 1.0,
    }
}

const fn key(action: KeyAction, width: f32) -> KeyDef {
    KeyDef { action, width }
}

/// English, the QWERTY key set from KOReader's `en_keyboard.lua`.
pub static ENGLISH: KeyboardLayout = KeyboardLayout {
    name: "en",
    labels: KeyLabels {
        shift: "Shift",
        caps: "Caps",
        delete: "Delete",
        enter: "Enter",
        space: "Space",
        hide: "Hide",
    },
    letters: &[
        &[
            ch('q'),
            ch('w'),
            ch('e'),
            ch('r'),
            ch('t'),
            ch('y'),
            ch('u'),
            ch('i'),
            ch('o'),
            ch('p'),
        ],
        &[
            ch('a'),
            ch('s'),
            ch('d'),
            ch('f'),
            ch('g'),
            ch('h'),
            ch('j'),
            ch('k'),
            ch('l'),
        ],
        &[
            key(KeyAction::Shift, 1.5),
            ch('z'),
            ch('x'),
            ch('c'),
            ch('v'),
            ch('b'),
            ch('n'),
            ch('m'),
            key(KeyAction::Backspace, 1.5),
        ],
        &[
            key(KeyAction::Layer(Layer::Numbers), 1.5),
            ch(','),
            key(KeyAction::Space, 3.5),
            ch('.'),
            key(KeyAction::Hide, 1.5),
            key(KeyAction::Enter, 1.5),
        ],
    ],
    numbers: &[
        &[
            ch('1'),
            ch('2'),
            ch('3'),
            ch('4'),
            ch('5'),
            ch('6'),
            ch('7'),
            ch('8'),
            ch('9'),
            ch('0'),
        ],
        &[
            ch('-'),
            ch('/'),
            ch(':'),
            ch(';'),
            ch('('),
            ch(')'),
            ch('$'),
            ch('&'),
            ch('@'),
            ch('"'),
        ],
        &[
            key(KeyAction::Layer(Layer::Symbols), 1.5),
            ch('+'),
            ch('*'),
            ch('='),
            ch('%'),
            ch('?'),
            ch('!'),
            ch('\''),
            key(KeyAction::Backspace, 1.5),
        ],
        &[
            key(KeyAction::Layer(Layer::Letters), 1.5),
            ch(','),
            key(KeyAction::Space, 3.5),
            ch('.'),
            key(KeyAction::Hide, 1.5),
            key(KeyAction::Enter, 1.5),
        ],
    ],
    symbols: &[
        &[
            ch('`'),
            ch('~'),
            ch('|'),
            ch('\\'),
            ch('{'),
            ch('}'),
            ch('['),
            ch(']'),
            ch('<'),
            ch('>'),
        ],
        &[
            ch('^'),
            ch('_'),
            ch('#'),
            ch('\''),
            ch('"'),
            ch(':'),
            ch(';'),
            ch('!'),
            ch('?'),
            ch('%'),
        ],
        &[
            key(KeyAction::Layer(Layer::Numbers), 1.5),
            ch('$'),
            ch('&'),
            ch('*'),
            ch('+'),
            ch('='),
            ch('('),
            ch(')'),
            key(KeyAction::Backspace, 1.5),
        ],
        &[
            key(KeyAction::Layer(Layer::Letters), 1.5),
            ch(','),
            key(KeyAction::Space, 3.5),
            ch('.'),
            key(KeyAction::Hide, 1.5),
            key(KeyAction::Enter, 1.5),
        ],
    ],
};
