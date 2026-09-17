//! Dictation languages offered in the UI.
//!
//! The empty code means auto-detect: Whisper identifies the language itself, which works well and
//! is the right default for someone who switches languages mid-day. Pinning a language helps when
//! auto-detect keeps guessing wrong (short utterances, heavy accent, lots of English loanwords),
//! and it also tells the clean-up model which language to write back.
//!
//! Codes are ISO-639-1, which is what the OpenAI-compatible `/audio/transcriptions` field expects.

/// (ISO-639-1 code, label shown in menus). Empty code = auto-detect.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("", "Auto-detect"),
    ("en", "English"),
    ("nl", "Nederlands"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("pl", "Polski"),
    ("sv", "Svenska"),
    ("da", "Dansk"),
    ("no", "Norsk"),
    ("fi", "Suomi"),
    ("tr", "Türkçe"),
    ("ru", "Русский"),
    ("uk", "Українська"),
    ("zh", "中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("hi", "हिन्दी"),
    ("ar", "العربية"),
];

/// Menu label for a code, falling back to the raw code for anything not in the list.
pub fn label(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| code.to_string())
}

/// English name used to instruct the clean-up model, e.g. "nl" -> "Dutch".
/// Returns None for auto-detect or an unknown code, in which case the model is told to keep
/// whatever language the speaker used.
pub fn english_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "en" => "English",
        "nl" => "Dutch",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "pl" => "Polish",
        "sv" => "Swedish",
        "da" => "Danish",
        "no" => "Norwegian",
        "fi" => "Finnish",
        "tr" => "Turkish",
        "ru" => "Russian",
        "uk" => "Ukrainian",
        "zh" => "Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "hi" => "Hindi",
        "ar" => "Arabic",
        _ => return None,
    })
}

/// Language-specific guidance appended to the clean-up system prompt.
///
/// The instructions stay in English because models follow English instructions most reliably, but
/// the filler words, punctuation conventions and worked example are the target language's own.
/// Generic languages get a short block; English and Dutch are written out properly.
pub fn cleanup_hint(code: &str) -> String {
    match code {
        "en" => "\n\nThe speaker dictates in English.\n\
             - English fillers to drop: um, uh, er, like, you know, I mean, sort of, kind of, basically, right (as a tag).\n\
             - Keep contractions as spoken (\"don't\", \"we'll\"); do not expand them.\n\
             - Capitalise days, months, languages and nationalities: Monday, January, Dutch.\n\
             Example\n\
             Transcript: \"\"\"um so I basically need to you know ship it by friday I mean thursday\"\"\"\n\
             {\"original\": \"um so I basically need to you know ship it by friday I mean thursday\", \"cleaned\": \"I need to ship it by Thursday.\"}"
            .to_string(),
        "nl" => "\n\nThe speaker dictates in Dutch (Nederlands).\n\
             - Dutch fillers to drop: eh, ehm, uhm, nou, nou ja, zeg maar, weet je (wel), dus (when it starts a sentence as a filler), gewoon and eigenlijk ONLY when they add nothing.\n\
             - Days, months and nationality adjectives are lower case in Dutch: maandag, januari, een nederlandse collega. Language names are capitalised: Nederlands, Engels.\n\
             - Write the IJ digraph with both letters capitalised at the start of a sentence or name: IJsselmeer, IJmuiden.\n\
             - Keep the elided article apostrophes: 's ochtends, 's avonds, 's-Hertogenbosch.\n\
             - Keep English loanwords that Dutch speakers genuinely use (deployen, de build, de meeting); do not translate them.\n\
             Example\n\
             Transcript: \"\"\"eh ja we moeten eh even kijken naar de planning van volgende week dinsdag nee woensdag\"\"\"\n\
             {\"original\": \"eh ja we moeten eh even kijken naar de planning van volgende week dinsdag nee woensdag\", \"cleaned\": \"Ja, we moeten even kijken naar de planning van volgende week woensdag.\"}"
            .to_string(),
        "" => "\n\nThe language is not pinned: detect it from the transcript and clean it using that \
               language's own punctuation, capitalisation and filler-word conventions."
            .to_string(),
        other => match english_name(other) {
            Some(name) => format!(
                "\n\nThe speaker usually dictates in {name}. Use {name}'s own conventions for \
                 punctuation, capitalisation and which words count as fillers."
            ),
            None => String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detect_is_first_and_has_no_code() {
        assert_eq!(LANGUAGES[0].0, "");
        assert_eq!(label(""), "Auto-detect");
        assert_eq!(english_name(""), None);
    }

    #[test]
    fn dutch_round_trips() {
        assert_eq!(label("nl"), "Nederlands");
        assert_eq!(english_name("nl"), Some("Dutch"));
    }

    #[test]
    fn hints_are_language_specific() {
        assert!(cleanup_hint("nl").contains("Nederlands"));
        assert!(cleanup_hint("nl").contains("IJsselmeer"));
        assert!(cleanup_hint("en").contains("English"));
        assert!(cleanup_hint("").contains("not pinned"));
        assert!(cleanup_hint("de").contains("German"));
        assert_eq!(cleanup_hint("xx"), "");
    }

    #[test]
    fn unknown_code_falls_back_to_itself() {
        assert_eq!(label("xx"), "xx");
        assert_eq!(english_name("xx"), None);
    }
}
