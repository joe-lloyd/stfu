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
    fn unknown_code_falls_back_to_itself() {
        assert_eq!(label("xx"), "xx");
        assert_eq!(english_name("xx"), None);
    }
}
