//! Manglish to Malayalam script.
//!
//! Apple's Malayalam speech model (`ml-IN`) writes what it hears in casual
//! Latin spelling ("kaalath ezhumani kazhinjittum nalla thanuppaayirunnu"),
//! the way people type Malayalam on a phone. This turns that spelling back
//! into Malayalam script, following the common conventions (Mozhi-style):
//! "th" is ത and "t" is ട, "zh" is ഴ, "nj" between vowels is ഞ്ഞ, "nt" is
//! ന്റ, doubled letters join with a virama, and a word ending on n, l, r or
//! m takes the chillu or anusvara.
//!
//! Casual spelling loses some distinctions (ന/ണ, ല/ള, ര/റ, long vowels the
//! model didn't hear), so the result reads naturally but is not always the
//! book's exact spelling. Acronyms in capitals ("DC", "TM") stay in Latin.

const VIRAMA: char = '\u{0D4D}';

/// Consonant spellings, longest first so "tth" wins over "th" and "t".
/// Entries marked medial apply only inside a word.
const CONSONANTS: &[(&str, &str, bool)] = &[
    ("ksh", "ക്ഷ", false),
    ("tth", "ത്ത", false),
    ("cch", "ച്ച", false),
    ("chh", "ഛ", false),
    ("nj", "ഞ്ഞ", true),
    ("ng", "ങ്ങ", true),
    ("nt", "ന്റ", true),
    ("nk", "ങ്ക", true),
    ("nd", "ണ്ട", true),
    ("mb", "മ്പ", true),
    ("kh", "ഖ", false),
    ("gh", "ഘ", false),
    ("ch", "ച", false),
    ("jh", "ഝ", false),
    ("nj", "ഞ", false),
    ("th", "ത", false),
    ("dh", "ധ", false),
    ("ph", "ഫ", false),
    ("bh", "ഭ", false),
    ("sh", "ശ", false),
    ("zh", "ഴ", false),
    ("rr", "റ", false),
    ("k", "ക", false),
    ("g", "ഗ", false),
    ("c", "ക", false),
    ("j", "ജ", false),
    ("t", "ട", false),
    // ദ is far more common than ഡ, which is mostly in English loanwords.
    ("d", "ദ", false),
    ("n", "ന", false),
    ("p", "പ", false),
    ("f", "ഫ", false),
    ("b", "ബ", false),
    ("m", "മ", false),
    ("y", "യ", false),
    ("r", "ര", false),
    ("l", "ല", false),
    ("v", "വ", false),
    ("w", "വ", false),
    ("s", "സ", false),
    ("h", "ഹ", false),
    ("z", "സ", false),
    ("q", "ക്വ", false),
    ("x", "ക്സ", false),
];

/// Vowel spellings: (Latin, standalone letter, sign after a consonant).
/// "a" after a consonant is the inherent vowel, so its sign is empty.
const VOWELS: &[(&str, &str, &str)] = &[
    ("aa", "ആ", "\u{0D3E}"),
    ("ee", "ഈ", "\u{0D40}"),
    ("ii", "ഈ", "\u{0D40}"),
    ("oo", "ഊ", "\u{0D42}"),
    ("uu", "ഊ", "\u{0D42}"),
    ("ai", "ഐ", "\u{0D48}"),
    ("au", "ഔ", "\u{0D57}"),
    ("ou", "ഔ", "\u{0D57}"),
    ("ae", "ഏ", "\u{0D47}"),
    ("a", "അ", ""),
    ("i", "ഇ", "\u{0D3F}"),
    ("u", "ഉ", "\u{0D41}"),
    ("e", "എ", "\u{0D46}"),
    ("o", "ഒ", "\u{0D4A}"),
];

/// Turns Apple's Manglish transcript into Malayalam script.
pub fn from_manglish(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    let mut word = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            word.push(ch);
        } else {
            flush(&mut word, &mut out);
            out.push(ch);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn flush(word: &mut String, out: &mut String) {
    if word.is_empty() {
        return;
    }
    // "DC", "TM", "ADC": acronyms read out letter by letter stay as they are.
    let acronym = word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase());
    if acronym {
        out.push_str(word);
    } else {
        out.push_str(&transliterate_word(&word.to_ascii_lowercase()));
    }
    word.clear();
}

fn transliterate_word(word: &str) -> String {
    let mut out = String::new();
    let mut rest = word;
    // The last consonant written with no vowel after it yet, and whether it
    // was joined onto a consonant before it.
    let mut pending: Option<(&str, bool)> = None;
    while !rest.is_empty() {
        let at_start = rest.len() == word.len();
        if let Some(&(latin, letter, _)) = CONSONANTS
            .iter()
            .find(|(latin, _, medial)| rest.starts_with(latin) && !(*medial && at_start))
        {
            // Two consonants in a row join with a virama: "kk" is ക്ക.
            let joined = pending.is_some();
            if joined {
                out.push(VIRAMA);
            }
            out.push_str(letter);
            pending = Some((letter, joined));
            rest = &rest[latin.len()..];
            continue;
        }
        if let Some(&(latin, standalone, sign)) =
            VOWELS.iter().find(|(latin, _, _)| rest.starts_with(latin))
        {
            if pending.take().is_some() {
                out.push_str(sign);
            } else {
                out.push_str(standalone);
            }
            rest = &rest[latin.len()..];
            continue;
        }
        // Anything else (shouldn't happen for ASCII letters): keep it.
        let ch = rest.chars().next().unwrap_or_default();
        out.push(ch);
        pending = None;
        rest = &rest[ch.len_utf8()..];
    }
    // A word ending on a consonant: chillu letters for n, l, r, anusvara for
    // m, otherwise a virama ("chuvapp" is ചുവപ്പ്).
    if let Some((letter, joined)) = pending {
        let ending = match letter {
            "ന" if !joined => Some('ൻ'),
            "ല" if !joined => Some('ൽ'),
            "ര" if !joined => Some('ർ'),
            "മ" if !joined => Some('ം'),
            _ => None,
        };
        match ending {
            Some(chillu) => {
                out.truncate(out.len() - letter.len());
                out.push(chillu);
            }
            None => out.push(VIRAMA),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::from_manglish;

    #[test]
    fn words_from_the_model() {
        let cases = [
            ("paccha", "പച്ച"),
            ("manja", "മഞ്ഞ"),
            ("chuvapp", "ചുവപ്പ്"),
            ("nalla", "നല്ല"),
            ("athinte", "അതിന്റെ"),
            ("malayil", "മലയിൽ"),
            ("ninnulla", "നിന്നുല്ല"),
            ("onnu", "ഒന്നു"),
            ("kazhinjittum", "കഴിഞ്ഞിട്ടും"),
            ("thanuppaayirunnu", "തനുപ്പായിരുന്നു"),
            ("koote", "കൂടെ"),
            ("shabdam", "ശബ്ദം"),
            ("rachana", "രചന"),
            ("kaalath", "കാലത്"),
            ("aayiratthi", "ആയിരത്തി"),
            ("konde", "കൊണ്ടെ"),
            ("randu", "രണ്ടു"),
            ("ambalam", "അമ്പലം"),
        ];
        for (latin, malayalam) in cases {
            assert_eq!(from_manglish(latin), malayalam, "{latin}");
        }
    }

    #[test]
    fn keeps_acronyms_spacing_and_punctuation() {
        assert_eq!(from_manglish("TD Ramakrishnan."), "TD രമക്രിശ്നൻ.");
        assert_eq!(from_manglish("nalla, DC books"), "നല്ല, DC ബൂക്സ്");
    }
}
