//! How text is cut into words for full-text search. Both what is stored and
//! what is queried go through `search_words`, so the two always agree.

#[derive(Clone, Copy, PartialEq)]
enum CharClass {
    Letter,
    Digit,
    Separator,
}

fn char_class(c: char) -> CharClass {
    if c.is_numeric() {
        CharClass::Digit
    } else if c.is_alphabetic() {
        CharClass::Letter
    } else {
        CharClass::Separator
    }
}

/// The runs of letters and of digits in `text`. Everything else separates
/// words, `_` included, and a word also ends where letters meet digits:
/// `utf8_decode` is `utf`, `8`, `decode`. Case is left alone; the search
/// table's tokenizer lowercases.
pub(super) fn search_words(text: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut word: Option<(usize, CharClass)> = None;
    for (i, c) in text.char_indices() {
        let class = char_class(c);
        if let Some((start, current)) = word {
            if current == class {
                continue;
            }
            words.push(&text[start..i]);
        }
        word = (class != CharClass::Separator).then_some((i, class));
    }
    if let Some((start, _)) = word {
        words.push(&text[start..]);
    }
    words
}
