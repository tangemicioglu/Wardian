//! DOM key identity to Windows virtual key code.
//!
//! `Input.dispatchKeyEvent` carries `key` and `code` for the page's benefit,
//! but Blink decides what a key *does* from `windowsVirtualKeyCode`. Without
//! it every editing key is inert: Backspace deletes nothing, the arrows do not
//! move the caret, and Enter neither submits a form nor inserts a newline.
//! Printable characters appear anyway, because a `keyDown` carrying `text`
//! synthesises the character event on its own — which is why a surface that
//! omits this looks like it accepts typing right up until the first
//! correction.
//!
//! The table is the US layout, matching what the protocol's own tooling sends.
//! A non-US physical layout still reports US-shaped `code` values for the keys
//! that matter here, so the editing keys stay correct even when the printable
//! ones would disagree — and printable keys carry their `text` regardless.

/// Windows virtual key code for a DOM `key`/`code` pair, when one is known.
///
/// `key` is consulted first so a named key (`Backspace`, `ArrowLeft`) resolves
/// the same way whatever produced it, then `code` for the physical keys whose
/// character depends on modifiers, and finally the printable character itself
/// for callers that supply no `code` at all.
pub fn virtual_key_code(key: &str, code: &str) -> Option<i64> {
    named_key_code(key)
        .or_else(|| physical_code(code))
        .or_else(|| printable_key_code(key))
}

/// Whether a key event should also carry text, and what that text is.
///
/// Enter is the exception worth spelling out: its `key` is a word rather than
/// the character it inserts, so without this a newline never reaches a
/// textarea.
pub fn key_text(key: &str) -> Option<&'static str> {
    match key {
        "Enter" => Some("\r"),
        "Tab" => Some("\t"),
        _ => None,
    }
}

fn named_key_code(key: &str) -> Option<i64> {
    let code = match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Clear" => 12,
        "Enter" => 13,
        "Shift" => 16,
        "Control" => 17,
        "Alt" => 18,
        "Pause" => 19,
        "CapsLock" => 20,
        "Escape" => 27,
        " " | "Spacebar" => 32,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "PrintScreen" => 44,
        "Insert" => 45,
        "Delete" => 46,
        "Meta" | "OS" => 91,
        "ContextMenu" => 93,
        "NumLock" => 144,
        "ScrollLock" => 145,
        _ => return function_key_code(key),
    };
    Some(code)
}

/// `F1` through `F24` are contiguous from 112.
fn function_key_code(key: &str) -> Option<i64> {
    let number: u32 = key.strip_prefix('F')?.parse().ok()?;
    if (1..=24).contains(&number) {
        Some(i64::from(111 + number))
    } else {
        None
    }
}

fn physical_code(code: &str) -> Option<i64> {
    if let Some(digit) = code.strip_prefix("Digit") {
        return single_ascii_digit(digit).map(i64::from);
    }
    if let Some(letter) = code.strip_prefix("Key") {
        return single_ascii_upper(letter).map(i64::from);
    }
    if let Some(digit) = code.strip_prefix("Numpad") {
        if let Some(value) = single_ascii_digit(digit) {
            // Numpad digits start at 96 rather than sharing the row's codes.
            return Some(i64::from(value) - 48 + 96);
        }
    }
    let value = match code {
        "NumpadMultiply" => 106,
        "NumpadAdd" => 107,
        "NumpadSubtract" => 109,
        "NumpadDecimal" => 110,
        "NumpadDivide" => 111,
        "NumpadEnter" => 13,
        "Semicolon" => 186,
        "Equal" => 187,
        "Comma" => 188,
        "Minus" => 189,
        "Period" => 190,
        "Slash" => 191,
        "Backquote" => 192,
        "BracketLeft" => 219,
        "Backslash" | "IntlBackslash" => 220,
        "BracketRight" => 221,
        "Quote" => 222,
        _ => return None,
    };
    Some(value)
}

/// Last resort for a caller that sent a bare character and no `code`.
fn printable_key_code(key: &str) -> Option<i64> {
    let mut characters = key.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    if character.is_ascii_alphanumeric() {
        return Some(i64::from(character.to_ascii_uppercase() as u8));
    }
    let value = match character {
        ' ' => 32,
        ';' | ':' => 186,
        '=' | '+' => 187,
        ',' | '<' => 188,
        '-' | '_' => 189,
        '.' | '>' => 190,
        '/' | '?' => 191,
        '`' | '~' => 192,
        '[' | '{' => 219,
        '\\' | '|' => 220,
        ']' | '}' => 221,
        '\'' | '"' => 222,
        '!' => 49,
        '@' => 50,
        '#' => 51,
        '$' => 52,
        '%' => 53,
        '^' => 54,
        '&' => 55,
        '*' => 56,
        '(' => 57,
        ')' => 48,
        _ => return None,
    };
    Some(value)
}

fn single_ascii_digit(value: &str) -> Option<u8> {
    let mut characters = value.chars();
    let character = characters.next()?;
    if characters.next().is_some() || !character.is_ascii_digit() {
        return None;
    }
    Some(character as u8)
}

fn single_ascii_upper(value: &str) -> Option<u8> {
    let mut characters = value.chars();
    let character = characters.next()?;
    if characters.next().is_some() || !character.is_ascii_alphabetic() {
        return None;
    }
    Some(character.to_ascii_uppercase() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editing_keys_that_looked_inert_now_resolve() {
        assert_eq!(virtual_key_code("Backspace", "Backspace"), Some(8));
        assert_eq!(virtual_key_code("Delete", "Delete"), Some(46));
        assert_eq!(virtual_key_code("Enter", "Enter"), Some(13));
        assert_eq!(virtual_key_code("Tab", "Tab"), Some(9));
        assert_eq!(virtual_key_code("ArrowLeft", "ArrowLeft"), Some(37));
        assert_eq!(virtual_key_code("Home", "Home"), Some(36));
        assert_eq!(virtual_key_code("Escape", "Escape"), Some(27));
    }

    #[test]
    fn a_shifted_character_resolves_through_its_physical_key() {
        // The character differs from the unshifted one; the code does not.
        assert_eq!(virtual_key_code("@", "Digit2"), Some(50));
        assert_eq!(virtual_key_code("A", "KeyA"), Some(65));
        assert_eq!(virtual_key_code("a", "KeyA"), Some(65));
        assert_eq!(virtual_key_code("_", "Minus"), Some(189));
    }

    #[test]
    fn a_bare_character_still_resolves_without_a_code() {
        assert_eq!(virtual_key_code("a", ""), Some(65));
        assert_eq!(virtual_key_code("7", ""), Some(55));
        assert_eq!(virtual_key_code("@", ""), Some(50));
        assert_eq!(virtual_key_code(" ", ""), Some(32));
    }

    #[test]
    fn the_numpad_does_not_borrow_the_number_row() {
        assert_eq!(virtual_key_code("1", "Numpad1"), Some(97));
        assert_eq!(virtual_key_code("1", "Digit1"), Some(49));
        assert_eq!(virtual_key_code("Enter", "NumpadEnter"), Some(13));
    }

    #[test]
    fn function_keys_span_their_whole_range() {
        assert_eq!(virtual_key_code("F1", "F1"), Some(112));
        assert_eq!(virtual_key_code("F12", "F12"), Some(123));
        assert_eq!(virtual_key_code("F24", "F24"), Some(135));
        assert_eq!(virtual_key_code("F25", "F25"), None);
        assert_eq!(virtual_key_code("Fnord", ""), None);
    }

    #[test]
    fn an_unknown_key_reports_nothing_rather_than_guessing() {
        assert_eq!(virtual_key_code("Unidentified", "Unknown"), None);
        assert_eq!(virtual_key_code("é", ""), None);
    }

    #[test]
    fn only_the_keys_whose_character_is_not_their_name_carry_text() {
        assert_eq!(key_text("Enter"), Some("\r"));
        assert_eq!(key_text("Tab"), Some("\t"));
        assert_eq!(key_text("Backspace"), None);
        assert_eq!(key_text("a"), None);
    }
}
