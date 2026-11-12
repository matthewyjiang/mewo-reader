//! Catalog genres and sorts. This table is the only list: the id clients send,
//! the name they show, and the Open Library subject keys behind it. Open
//! Library filters only inside `q`, and `subject_key:"x"` is an exact match.

pub(crate) struct Genre {
    pub id: &'static str,
    pub name: &'static str,
    pub keys: &'static [&'static str],
    /// Subject keys a work must not carry, such as `fiction` for Biography.
    pub not: &'static [&'static str],
}

pub(crate) const GENRES: &[Genre] = &[
    Genre {
        id: "fantasy",
        name: "Fantasy",
        keys: &["fantasy", "fantasy_fiction", "fiction_fantasy_general"],
        not: &[],
    },
    Genre {
        id: "science-fiction",
        name: "Science fiction",
        keys: &["science_fiction", "fiction_science_fiction_general"],
        not: &[],
    },
    Genre {
        id: "mystery",
        name: "Mystery",
        keys: &[
            "detective_and_mystery_stories",
            "mystery_fiction",
            "fiction_mystery__detective_general",
        ],
        not: &[],
    },
    Genre {
        id: "thriller",
        name: "Thriller",
        keys: &[
            "thrillers",
            "fiction_thrillers_general",
            "fiction_thrillers_suspense",
            "suspense",
        ],
        not: &[],
    },
    Genre {
        id: "romance",
        name: "Romance",
        keys: &["romance_fiction", "love_stories", "fiction_romance_general"],
        not: &[],
    },
    Genre {
        id: "horror",
        name: "Horror",
        keys: &[
            "horror_fiction",
            "horror_stories",
            "horror_tales",
            "fiction_horror",
        ],
        not: &[],
    },
    Genre {
        id: "historical-fiction",
        name: "Historical fiction",
        keys: &["historical_fiction", "fiction_historical_general"],
        not: &[],
    },
    Genre {
        id: "young-adult",
        name: "Young adult",
        keys: &["young_adult_fiction"],
        not: &[],
    },
    Genre {
        id: "biography",
        name: "Biography",
        keys: &["biography", "autobiography", "biographies", "memoirs"],
        not: &["fiction"],
    },
    Genre {
        id: "history",
        name: "History",
        keys: &["history"],
        not: &["fiction"],
    },
    Genre {
        id: "self-help",
        name: "Self-help",
        keys: &["self-help", "self_help"],
        not: &[],
    },
    Genre {
        id: "poetry",
        name: "Poetry",
        keys: &["poetry"],
        not: &[],
    },
];

pub(crate) fn find(id: &str) -> Option<&'static Genre> {
    GENRES.iter().find(|genre| genre.id == id)
}

impl Genre {
    /// `(subject_key:"a" OR subject_key:"b") -subject_key:"fiction"`.
    pub(crate) fn clause(&self) -> String {
        let any = self
            .keys
            .iter()
            .map(|key| format!("subject_key:\"{key}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut clause = format!("({any})");
        for key in self.not {
            clause.push_str(&format!(" -subject_key:\"{key}\""));
        }
        clause
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sort {
    Popular,
    Rated,
    New,
}

impl Sort {
    pub(crate) const ALL: [Sort; 3] = [Sort::Popular, Sort::Rated, Sort::New];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Sort::Popular => "popular",
            Sort::Rated => "rated",
            Sort::New => "new",
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Sort::Popular => "Popular",
            Sort::Rated => "Top rated",
            Sort::New => "New",
        }
    }

    pub(crate) fn parse(id: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|sort| sort.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn genre_ids_are_unique_kebab_case_and_every_genre_has_keys() {
        let mut seen = HashSet::new();
        for genre in GENRES {
            assert!(seen.insert(genre.id), "duplicate id {}", genre.id);
            assert!(
                !genre.id.is_empty()
                    && !genre.id.starts_with('-')
                    && !genre.id.ends_with('-')
                    && !genre.id.contains("--")
                    && genre.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{} is not kebab-case",
                genre.id
            );
            assert!(!genre.keys.is_empty(), "{} has no keys", genre.id);
            assert!(!genre.name.is_empty());
            assert_eq!(find(genre.id).map(|found| found.name), Some(genre.name));
        }
        assert_eq!(GENRES.len(), 12);
        assert!(find("Fantasy").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn clause_quotes_every_key_and_excludes_the_not_keys() {
        assert_eq!(
            find("science-fiction").unwrap().clause(),
            r#"(subject_key:"science_fiction" OR subject_key:"fiction_science_fiction_general")"#
        );
        assert_eq!(
            find("history").unwrap().clause(),
            r#"(subject_key:"history") -subject_key:"fiction""#
        );
        assert_eq!(
            find("self-help").unwrap().clause(),
            r#"(subject_key:"self-help" OR subject_key:"self_help")"#
        );
    }

    #[test]
    fn sort_parses_its_own_ids_only() {
        for sort in Sort::ALL {
            assert_eq!(Sort::parse(sort.id()), Some(sort));
        }
        assert_eq!(Sort::parse("rated"), Some(Sort::Rated));
        assert_eq!(Sort::parse("Popular"), None);
        assert_eq!(Sort::parse("rating"), None);
        assert_eq!(Sort::parse(""), None);
        assert_eq!(Sort::ALL.map(Sort::name), ["Popular", "Top rated", "New"]);
    }
}
