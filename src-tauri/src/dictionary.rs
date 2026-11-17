//! Personal dictionary: words the transcriber keeps getting wrong (a colleague called Koen comes
//! out as "Kuhn", "Koon" or "Cohen") and words that must be written one particular way ("GitHub").
//!
//! An entry is applied at three points of a take:
//! 1. its word goes to the transcriber as a vocabulary hint (Whisper's `prompt`), which gets the
//!    spelling right some of the time;
//! 2. the transcript is resolved here: every "sounds like" spelling becomes the word, and with
//!    `fix_case` any other capitalisation of the word itself is corrected;
//! 3. the clean-up model is told the word and its known mishearings, so it can also fix a
//!    spelling that is not listed yet; its reply is resolved once more.
//!
//! Step 2 is plain text replacement on whole words, so it works with clean-up switched off, when
//! clean-up fails, and offline.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The word exactly as it should be written, e.g. "Koen" or "GitHub". May be several words.
    pub word: String,
    /// Spellings the transcriber produces instead, e.g. "Kuhn", "Koon". Replaced wherever they
    /// appear as whole words, in any capitalisation.
    #[serde(default)]
    pub sounds_like: Vec<String>,
    /// Also correct the capitalisation of the word itself ("github" becomes "GitHub"). Switch off
    /// for a name that is also an everyday word, like Will or Mark.
    #[serde(default = "yes")]
    pub fix_case: bool,
}

fn yes() -> bool {
    true
}

/// One replacement made in a take, kept in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fix {
    pub from: String,
    pub to: String,
}

/// Trims every field, drops empty words and duplicate spellings, and drops "sounds like" spellings
/// that are just the word itself. Entries keep their order.
pub fn normalize(entries: Vec<Entry>) -> Vec<Entry> {
    let mut seen_words = HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        let word = squash(&e.word);
        if word.is_empty() || !seen_words.insert(word.to_lowercase()) {
            continue;
        }
        let mut seen = HashSet::from([word.to_lowercase()]);
        let sounds_like = e
            .sounds_like
            .iter()
            .flat_map(|s| s.split(','))
            .map(squash)
            .filter(|s| !s.is_empty() && seen.insert(s.to_lowercase()))
            .collect();
        out.push(Entry { word, sounds_like, fix_case: e.fix_case });
    }
    out
}

/// Collapses runs of whitespace to one space and trims.
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct Pattern {
    /// The phrase to find, one char vector per word.
    words: Vec<Vec<char>>,
    to: String,
}

impl Pattern {
    fn len(&self) -> usize {
        self.words.iter().map(Vec::len).sum::<usize>() + self.words.len()
    }
}

fn patterns(entries: &[Entry]) -> Vec<Pattern> {
    let mut out = Vec::new();
    for e in entries {
        let word = squash(&e.word);
        if word.is_empty() {
            continue;
        }
        let from = e.sounds_like.iter().map(|s| squash(s)).filter(|s| !s.is_empty());
        let own = e.fix_case.then(|| word.clone());
        for phrase in from.chain(own) {
            out.push(Pattern {
                words: phrase.split(' ').map(|w| w.chars().collect()).collect(),
                to: word.clone(),
            });
        }
    }
    // Longest first, so "hyper solid" wins over "hyper" when both are listed.
    out.sort_by_key(|p| std::cmp::Reverse(p.len()));
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

fn same(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Where a match of `p` starting at `i` ends, and how many of those chars are a plural or
/// possessive "s" to keep ("Kuhns" -> "Koens"). Words of a phrase may be split by spaces or
/// hyphens in the text.
fn match_at(text: &[char], i: usize, p: &Pattern) -> Option<(usize, usize)> {
    if i > 0 && is_word_char(text[i - 1]) {
        return None;
    }
    let mut j = i;
    for (k, word) in p.words.iter().enumerate() {
        if k > 0 {
            let start = j;
            while j < text.len() && (text[j].is_whitespace() || text[j] == '-') {
                j += 1;
            }
            if j == start {
                return None;
            }
        }
        if text.len() < j + word.len() || !word.iter().zip(&text[j..]).all(|(a, b)| same(*a, *b)) {
            return None;
        }
        j += word.len();
    }
    let ends_word = |at: usize| at >= text.len() || !is_word_char(text[at]);
    if ends_word(j) {
        return Some((j, 0));
    }
    if matches!(text[j], 's' | 'S') && ends_word(j + 1) {
        return Some((j + 1, 1));
    }
    None
}

/// Applies the dictionary to a text: every listed mishearing becomes its word, and words with
/// `fix_case` get their capitalisation. Returns the new text and what was changed.
pub fn resolve(text: &str, entries: &[Entry]) -> (String, Vec<Fix>) {
    let pats = patterns(entries);
    if pats.is_empty() {
        return (text.to_string(), Vec::new());
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut fixes = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let hit = pats.iter().find_map(|p| match_at(&chars, i, p).map(|m| (p, m)));
        match hit {
            Some((p, (end, suffix))) => {
                let from: String = chars[i..end].iter().collect();
                let to = format!("{}{}", p.to, chars[end - suffix..end].iter().collect::<String>());
                if from != to {
                    fixes.push(Fix { from, to: to.clone() });
                }
                out.push_str(&to);
                i = end;
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    (out, fixes)
}

/// Whisper's prompt is limited to 224 tokens; stay well inside it.
const PROMPT_CHARS: usize = 600;

/// The vocabulary hint for the transcriber: the words, comma-separated, in the style Whisper's
/// prompt works best with. `None` with an empty dictionary.
pub fn stt_prompt(entries: &[Entry]) -> Option<String> {
    let mut prompt = String::new();
    for e in entries {
        let word = squash(&e.word);
        if word.is_empty() {
            continue;
        }
        if prompt.len() + word.len() + 2 > PROMPT_CHARS {
            break;
        }
        if !prompt.is_empty() {
            prompt.push_str(", ");
        }
        prompt.push_str(&word);
    }
    (!prompt.is_empty()).then(|| format!("{prompt}."))
}

fn word_set(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// True when the transcriber, given nothing to hear, wrote back the vocabulary hint itself, which
/// Whisper is known to do on silence. Only a take that repeats the whole list counts, so dictating
/// a single name from it still works.
pub fn is_prompt_echo(transcript: &str, prompt: &str) -> bool {
    let hint = word_set(prompt);
    let said = word_set(transcript);
    hint.len() >= 2 && said.is_superset(&hint) && hint.is_superset(&said)
}

/// The clean-up model's part: the words, how to spell them, and how they tend to be misheard.
pub fn llm_section(entries: &[Entry]) -> String {
    let lines: Vec<String> = entries
        .iter()
        .filter(|e| !e.word.trim().is_empty())
        .map(|e| {
            let heard: Vec<&str> = e.sounds_like.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
            if heard.is_empty() {
                format!("- {}", e.word.trim())
            } else {
                format!("- {} (misheard as: {})", e.word.trim(), heard.join(", "))
            }
        })
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "\n\nPersonal dictionary. The speaker uses these words; always write them exactly as \
         spelled here, with this capitalisation. The transcriber often mishears them: a word in \
         the transcript that sounds like one of them (the listed mishearings, or a similar \
         spelling) is that word whenever the context fits, so write the dictionary spelling \
         instead. Possessives and plurals follow normally (Koen's).\n{}",
        lines.join("\n")
    )
}

/// A spelling seen in past transcripts that sounds like a dictionary word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub text: String,
    pub count: usize,
}

/// Spellings in `transcripts` that sound like `word`, most frequent first: capitalised words in
/// the middle of a sentence (that is how a misheard name shows up) whose sound matches. Ones
/// already in `known` are left out.
pub fn suggest<'a>(word: &str, transcripts: impl Iterator<Item = &'a str>, known: &[String]) -> Vec<Suggestion> {
    let word = squash(word);
    if word.is_empty() || word.contains(' ') {
        return Vec::new();
    }
    let target = phonetic_key(&word);
    if target.len() < 2 {
        return Vec::new();
    }
    let known: HashSet<String> = known.iter().chain([&word]).map(|k| k.to_lowercase()).collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for text in transcripts {
        for token in name_like_tokens(text) {
            let stem = token.strip_suffix('s').filter(|s| s.chars().count() >= 3);
            let found = if sounds_alike(&target, &phonetic_key(token)) {
                Some(token)
            } else {
                stem.filter(|s| sounds_alike(&target, &phonetic_key(s)))
            };
            if let Some(found) = found.filter(|f| !known.contains(&f.to_lowercase())) {
                *counts.entry(found.to_string()).or_default() += 1;
            }
        }
    }
    let mut out: Vec<Suggestion> = counts.into_iter().map(|(text, count)| Suggestion { text, count }).collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.text.cmp(&b.text)));
    out.truncate(12);
    out
}

/// Capitalised words that do not start a sentence, with any possessive "'s" removed.
fn name_like_tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut sentence_start = true;
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| c.is_alphanumeric()) {
        let before = &rest[..start];
        if before.contains(['.', '!', '?', ':', '\n', '"']) {
            sentence_start = true;
        }
        let after = &rest[start..];
        let len = after
            .find(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
            .unwrap_or(after.len());
        let token = &after[..len];
        let token = token.trim_end_matches(['\'', '’']);
        let token = token.strip_suffix("'s").or_else(|| token.strip_suffix("’s")).unwrap_or(token);
        let capital = token.chars().next().is_some_and(char::is_uppercase);
        if capital && !sentence_start && token.chars().count() >= 2 && token.chars().all(char::is_alphabetic) {
            out.push(token);
        }
        sentence_start = false;
        rest = &after[len..];
    }
    out
}

/// Two-sound keys must match exactly. Longer ones may differ by one sound, but a three-sound key
/// must still start the same way, or every short word in the history would qualify.
fn sounds_alike(target: &str, key: &str) -> bool {
    if key.len() < 2 {
        return false;
    }
    if target.len() <= 2 {
        return key == target;
    }
    if target.len() == 3 && target.chars().next() != key.chars().next() {
        return false;
    }
    edit_distance(target, key) <= 1
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn fold(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

/// A rough sound of a word: consonant classes in order, vowels dropped after the first letter,
/// repeats collapsed. "Koen", "Kuhn", "Cohen", "Koon" and "Kun" all come out as "KN".
fn phonetic_key(word: &str) -> String {
    let w: Vec<char> = word
        .to_lowercase()
        .chars()
        .map(fold)
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let mut out = String::new();
    let push = |code: char, out: &mut String| {
        if !out.ends_with(code) {
            out.push(code);
        }
    };
    let mut i = 0;
    while i < w.len() {
        let c = w[i];
        let next = w.get(i + 1).copied();
        let mut step = 1;
        let code = match (c, next) {
            ('p', Some('h')) => {
                step = 2;
                Some('F')
            }
            ('s' | 'c', Some('h')) => {
                step = 2;
                Some('X')
            }
            ('t', Some('h')) => {
                step = 2;
                Some('T')
            }
            ('c', Some('k')) => {
                step = 2;
                Some('K')
            }
            ('c', Some('e' | 'i' | 'y')) => Some('S'),
            ('c' | 'k' | 'q', _) => Some('K'),
            ('x', _) => {
                push('K', &mut out);
                Some('S')
            }
            ('z', _) => Some('S'),
            ('v', _) => Some('F'),
            ('a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w', _) => (i == 0).then_some(c.to_ascii_uppercase()),
            (other, _) => Some(other.to_ascii_uppercase()),
        };
        if let Some(code) = code {
            push(code, &mut out);
        }
        i += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn koen() -> Entry {
        Entry {
            word: "Koen".into(),
            sounds_like: ["Kuhn", "Koon", "Coon", "Cohen", "Kun"].map(String::from).to_vec(),
            fix_case: true,
        }
    }

    #[test]
    fn misheard_names_become_the_dictionary_word() {
        let (out, fixes) = resolve("Then you can assign Koon, Pieter and Sanne. Ask kuhn too.", &[koen()]);
        assert_eq!(out, "Then you can assign Koen, Pieter and Sanne. Ask Koen too.");
        assert_eq!(fixes, vec![
            Fix { from: "Koon".into(), to: "Koen".into() },
            Fix { from: "kuhn".into(), to: "Koen".into() },
        ]);
    }

    #[test]
    fn possessives_and_plurals_carry_over() {
        let (out, _) = resolve("all of Kun's work, and Coons updates", &[koen()]);
        assert_eq!(out, "all of Koen's work, and Koens updates");
    }

    #[test]
    fn only_whole_words_are_replaced() {
        let (out, fixes) = resolve("Kunming and Kuhnert stay, Raccoon too", &[koen()]);
        assert_eq!(out, "Kunming and Kuhnert stay, Raccoon too");
        assert!(fixes.is_empty());
    }

    #[test]
    fn capitalisation_is_fixed_unless_switched_off() {
        let gh = Entry { word: "GitHub".into(), sounds_like: vec![], fix_case: true };
        assert_eq!(resolve("push to github now", &[gh.clone()]).0, "push to GitHub now");
        let will = Entry { word: "Will".into(), sounds_like: vec!["Wil".into()], fix_case: false };
        assert_eq!(resolve("it will work, ask wil", &[will]).0, "it will work, ask Will");
        let (_, fixes) = resolve("GitHub", &[gh]);
        assert!(fixes.is_empty(), "already right is not a fix");
    }

    #[test]
    fn phrases_match_across_spaces_and_hyphens_longest_first() {
        let entries = vec![
            Entry { word: "Type".into(), sounds_like: vec!["tipe".into()], fix_case: false },
            Entry { word: "TypeScript".into(), sounds_like: vec!["type script".into()], fix_case: true },
        ];
        assert_eq!(resolve("it is in type-script, tipe", &entries).0, "it is in TypeScript, Type");
        assert_eq!(resolve("Type  Script", &entries).0, "TypeScript");
    }

    #[test]
    fn normalize_cleans_up_what_settings_sends() {
        let raw = vec![
            Entry { word: "  Koen ".into(), sounds_like: vec!["Kuhn, Koon".into(), "kuhn".into(), "koen".into(), " ".into()], fix_case: true },
            Entry { word: "koen".into(), sounds_like: vec![], fix_case: true },
            Entry { word: "".into(), sounds_like: vec!["x".into()], fix_case: true },
        ];
        assert_eq!(normalize(raw), vec![Entry {
            word: "Koen".into(),
            sounds_like: vec!["Kuhn".into(), "Koon".into()],
            fix_case: true,
        }]);
    }

    #[test]
    fn entries_without_fix_case_default_to_it() {
        let e: Entry = serde_json::from_str(r#"{"word": "Koen"}"#).unwrap();
        assert!(e.fix_case);
        assert!(e.sounds_like.is_empty());
    }

    #[test]
    fn the_prompt_lists_the_words_and_its_echo_is_caught() {
        let entries = vec![koen(), Entry { word: "Kubernetes".into(), sounds_like: vec![], fix_case: true }];
        let prompt = stt_prompt(&entries).unwrap();
        assert_eq!(prompt, "Koen, Kubernetes.");
        assert!(stt_prompt(&[]).is_none());
        assert!(is_prompt_echo(" Koen, Kubernetes.", &prompt));
        assert!(!is_prompt_echo("Koen.", &prompt), "a single name is real dictation");
        assert!(!is_prompt_echo("Ask Koen about Kubernetes.", &prompt));
    }

    #[test]
    fn the_model_is_told_the_words_and_their_mishearings() {
        let s = llm_section(&[koen()]);
        assert!(s.contains("- Koen (misheard as: Kuhn, Koon, Coon, Cohen, Kun)"));
        assert_eq!(llm_section(&[]), "");
    }

    #[test]
    fn real_mishearings_of_a_name_sound_alike() {
        for heard in ["Kuhn", "Koon", "Coon", "Cohen", "Kun", "Cun", "Cooen", "Cone"] {
            assert_eq!(phonetic_key(heard), phonetic_key("Koen"), "{heard}");
        }
        assert_eq!(phonetic_key("Charlotte"), phonetic_key("Sharlotte"));
        assert!(sounds_alike(&phonetic_key("Gerrit"), &phonetic_key("Garrett")));
        assert!(sounds_alike(&phonetic_key("Kubernetes"), &phonetic_key("Cubernetes")));
        assert!(sounds_alike(&phonetic_key("Arne"), &phonetic_key("Anno")));
        assert!(!sounds_alike(&phonetic_key("Arne"), &phonetic_key("Journey")), "short keys start alike");
        assert!(!sounds_alike(&phonetic_key("Koen"), &phonetic_key("Charlotte")));
    }

    #[test]
    fn suggestions_come_from_mid_sentence_capitalised_words() {
        let history = [
            "Then you can assign Koon, Pieter, and Sanne as reviewers.",
            "Let's add Cohen, Pieter, and Sanne. Can you ask Kuhn?",
            "all of Kun's stuff and Coons updates. Kuhn will check.",
            "Can we go? I can see it.",
        ];
        let got = suggest("Koen", history.iter().copied(), &["Kun".into()]);
        let names: Vec<(&str, usize)> = got.iter().map(|s| (s.text.as_str(), s.count)).collect();
        // "Kuhn" opening a sentence does not count, "Coons" counts as "Coon", "Kun" is already
        // listed, and "Can" never counts (lowercase or sentence-initial).
        assert_eq!(names, vec![("Cohen", 1), ("Coon", 1), ("Koon", 1), ("Kuhn", 1)]);
    }
}
