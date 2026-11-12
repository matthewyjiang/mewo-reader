//! Languages a request may download. This table is the only list: ISO 639-1
//! code, the ISO 639-2 forms Calibre writes, the English word in a release
//! name, the name the settings screen shows, and the MARC code Open Library
//! search filters on.

pub(crate) struct Language {
    pub code: &'static str,
    pub aliases: &'static [&'static str],
    pub word: &'static str,
    pub name: &'static str,
    pub marc: &'static str,
}

pub(crate) const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        aliases: &["eng"],
        word: "english",
        name: "English",
        marc: "eng",
    },
    Language {
        code: "es",
        aliases: &["spa"],
        word: "spanish",
        name: "Spanish",
        marc: "spa",
    },
    Language {
        code: "fr",
        aliases: &["fra", "fre"],
        word: "french",
        name: "French",
        marc: "fre",
    },
    Language {
        code: "de",
        aliases: &["deu", "ger"],
        word: "german",
        name: "German",
        marc: "ger",
    },
    Language {
        code: "it",
        aliases: &["ita"],
        word: "italian",
        name: "Italian",
        marc: "ita",
    },
    Language {
        code: "pt",
        aliases: &["por"],
        word: "portuguese",
        name: "Portuguese",
        marc: "por",
    },
    Language {
        code: "nl",
        aliases: &["nld", "dut"],
        word: "dutch",
        name: "Dutch",
        marc: "dut",
    },
    Language {
        code: "sv",
        aliases: &["swe"],
        word: "swedish",
        name: "Swedish",
        marc: "swe",
    },
    Language {
        code: "pl",
        aliases: &["pol"],
        word: "polish",
        name: "Polish",
        marc: "pol",
    },
    Language {
        code: "ru",
        aliases: &["rus"],
        word: "russian",
        name: "Russian",
        marc: "rus",
    },
    Language {
        code: "ja",
        aliases: &["jpn"],
        word: "japanese",
        name: "Japanese",
        marc: "jpn",
    },
    Language {
        code: "zh",
        aliases: &["zho", "chi"],
        word: "chinese",
        name: "Chinese",
        marc: "chi",
    },
    Language {
        code: "ko",
        aliases: &["kor"],
        word: "korean",
        name: "Korean",
        marc: "kor",
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

/// Open Library `language:` filter for the accepted codes, in table order:
/// `language:eng` or `language:(eng OR fre)`. `fra`, `deu`, and `nld` match
/// nothing there, so this uses the MARC codes.
pub(crate) fn marc_clause(accepted: &[String]) -> Option<String> {
    let marcs: Vec<&str> = LANGUAGES
        .iter()
        .filter(|language| accepted.iter().any(|code| code == language.code))
        .map(|language| language.marc)
        .collect();
    match marcs.as_slice() {
        [] => None,
        [one] => Some(format!("language:{one}")),
        many => Some(format!("language:({})", many.join(" OR "))),
    }
}

/// ISO 639-1 code when `token` is a supported language's English word.
pub(crate) fn word_code(token: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|language| language.word == token)
        .map(|language| language.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(list: &[&str]) -> Vec<String> {
        list.iter().map(|code| code.to_string()).collect()
    }

    #[test]
    fn marc_clause_names_one_or_many_in_table_order() {
        assert_eq!(
            marc_clause(&codes(&["en"])).as_deref(),
            Some("language:eng")
        );
        assert_eq!(
            marc_clause(&codes(&["fr", "en"])).as_deref(),
            Some("language:(eng OR fre)")
        );
        assert_eq!(
            marc_clause(&codes(&["de", "nl", "zh"])).as_deref(),
            Some("language:(ger OR dut OR chi)")
        );
        assert_eq!(marc_clause(&[]), None);
    }

    #[test]
    fn every_marc_code_is_an_alias_of_its_language() {
        for language in LANGUAGES {
            assert!(
                language.aliases.contains(&language.marc),
                "{} marc {}",
                language.code,
                language.marc
            );
        }
    }
}
