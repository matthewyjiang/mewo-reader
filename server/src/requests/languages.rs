//! Languages a request may download. This table is the only list: ISO 639-1
//! code, the ISO 639-2 forms Calibre writes, the English word in a release
//! name, and the name the settings screen shows.

pub(crate) struct Language {
    pub code: &'static str,
    pub aliases: &'static [&'static str],
    pub word: &'static str,
    pub name: &'static str,
}

pub(crate) const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        aliases: &["eng"],
        word: "english",
        name: "English",
    },
    Language {
        code: "es",
        aliases: &["spa"],
        word: "spanish",
        name: "Spanish",
    },
    Language {
        code: "fr",
        aliases: &["fra", "fre"],
        word: "french",
        name: "French",
    },
    Language {
        code: "de",
        aliases: &["deu", "ger"],
        word: "german",
        name: "German",
    },
    Language {
        code: "it",
        aliases: &["ita"],
        word: "italian",
        name: "Italian",
    },
    Language {
        code: "pt",
        aliases: &["por"],
        word: "portuguese",
        name: "Portuguese",
    },
    Language {
        code: "nl",
        aliases: &["nld", "dut"],
        word: "dutch",
        name: "Dutch",
    },
    Language {
        code: "sv",
        aliases: &["swe"],
        word: "swedish",
        name: "Swedish",
    },
    Language {
        code: "pl",
        aliases: &["pol"],
        word: "polish",
        name: "Polish",
    },
    Language {
        code: "ru",
        aliases: &["rus"],
        word: "russian",
        name: "Russian",
    },
    Language {
        code: "ja",
        aliases: &["jpn"],
        word: "japanese",
        name: "Japanese",
    },
    Language {
        code: "zh",
        aliases: &["zho", "chi"],
        word: "chinese",
        name: "Chinese",
    },
    Language {
        code: "ko",
        aliases: &["kor"],
        word: "korean",
        name: "Korean",
    },
];

pub(crate) fn default_codes() -> Vec<String> {
    vec!["en".to_string()]
}

pub(crate) fn is_code(code: &str) -> bool {
    LANGUAGES.iter().any(|language| language.code == code)
}

/// `und`, `mul`, and `zxx` are no language. Otherwise the tag has to name an
/// accepted language: its code or 3-letter alias (a primary subtag such as
/// `en-US`, `en_GB`, or `ger`), or that language's English word or display
/// name (`English`, `english`).
pub(crate) fn accepts(accepted: &[String], tag: &str) -> bool {
    let lower = tag.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "und" | "mul" | "zxx") {
        return true;
    }
    let primary = lower.split(['-', '_']).next().unwrap_or(lower.as_str());
    LANGUAGES.iter().any(|language| {
        accepted.iter().any(|code| code == language.code)
            && (language.code == primary
                || language.aliases.contains(&primary)
                || language.word == lower
                || language.name.eq_ignore_ascii_case(&lower))
    })
}

/// ISO 639-1 code when `token` is a supported language's English word.
pub(crate) fn word_code(token: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|language| language.word == token)
        .map(|language| language.code)
}
