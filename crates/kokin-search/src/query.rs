//! Turning what a person typed into an expression FTS5 will accept.
//!
//! This module exists because a search box is an untrusted input that gets
//! handed to a query language. FTS5's `MATCH` syntax has operators (`AND`,
//! `OR`, `NOT`, `NEAR`), phrase quoting, prefix stars, parentheses, and column
//! filters (`title : foo`). Passing a user's raw text through means:
//!
//! * an unbalanced quote or a stray `*` is a syntax error, so the search box
//!   breaks on inputs as ordinary as `O'Brien "unclosed` or `C++`;
//! * a typed `NOT` or a `title :` silently changes what was asked for, and the
//!   result is a *shorter* result list that looks like a finding about the
//!   world rather than a misparse.
//!
//! The second is the one that matters here. A tool whose searches quietly mean
//! something other than what was typed will, sooner or later, have someone
//! conclude a person is not in a case when they are.
//!
//! So user text never reaches FTS5 as syntax. Every chunk of it is wrapped in
//! double quotes, which makes it a *phrase*: FTS5 tokenises the contents and
//! matches them in order, and every operator character inside becomes literal
//! text for the tokeniser to discard. Structure comes only from what this
//! module decides — never from the characters the user happened to type.
//!
//! The one thing quoting cannot fix is a chunk that holds no tokens at all
//! (`---`, `@@@`, `...`). FTS5 rejects the empty phrase `""` as a syntax
//! error, so those chunks are dropped before they are ever emitted.

/// A user's query, parsed into what it asks for.
///
/// Constructed only through [`Query::parse`] and friends, so a `Query` is by
/// construction something that can be rendered safely.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    positives: Vec<Chunk>,
    negatives: Vec<Chunk>,
    prefix_last: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    text: String,
    /// Whether the user quoted this themselves. A chunk the user quoted is an
    /// exact phrase they asked for, so typeahead must not extend it.
    quoted: bool,
}

impl Query {
    /// Parse a query the user has finished typing.
    pub fn parse(raw: &str) -> Self {
        Self::parse_inner(raw, false)
    }

    /// Parse a query the user is still typing, treating the final word as a
    /// prefix so results narrow as they type.
    ///
    /// Applied only to a chunk the user did not quote: extending `"acme ltd"`
    /// to `"acme ltd"*` would answer a question they did not ask.
    pub fn parse_for_typeahead(raw: &str) -> Self {
        Self::parse_inner(raw, true)
    }

    fn parse_inner(raw: &str, prefix_last: bool) -> Self {
        let mut positives = Vec::new();
        let mut negatives = Vec::new();

        for (negated, chunk) in chunks(raw) {
            // A chunk with nothing tokenisable in it would render as the empty
            // phrase, which FTS5 refuses. `is_alphanumeric` approximates
            // unicode61's token definition (Unicode letters and numbers) - the
            // approximation is safe in the only direction that matters: it
            // never keeps a chunk the tokeniser would find empty.
            if !chunk.text.chars().any(char::is_alphanumeric) {
                continue;
            }
            if negated {
                negatives.push(chunk);
            } else {
                positives.push(chunk);
            }
        }

        Self {
            positives,
            negatives,
            prefix_last,
        }
    }

    /// True if this query asks for nothing findable.
    pub fn is_empty(&self) -> bool {
        self.positives.is_empty()
    }

    /// The FTS5 `MATCH` expression, or `None` if there is nothing to match.
    ///
    /// `None` covers two cases that both have to mean "no results" rather than
    /// "all results": an empty box, and a query with only exclusions. FTS5
    /// cannot express "everything except x" - `NOT x` has no left-hand side -
    /// and answering it with an unfiltered scan of the whole case would be the
    /// opposite of what was asked.
    pub fn to_fts5(&self) -> Option<String> {
        if self.positives.is_empty() {
            return None;
        }

        let mut parts: Vec<String> = self.positives.iter().map(|c| phrase(&c.text)).collect();

        if self.prefix_last {
            // Safe: `positives` is non-empty, checked above.
            if let (Some(last), Some(chunk)) = (parts.last_mut(), self.positives.last()) {
                if !chunk.quoted {
                    last.push('*');
                }
            }
        }

        let mut expression = parts.join(" AND ");

        if !self.negatives.is_empty() {
            let excluded = self
                .negatives
                .iter()
                .map(|c| phrase(&c.text))
                .collect::<Vec<_>>()
                .join(" OR ");
            // Parenthesised on both sides. FTS5's precedence would group this
            // correctly anyway, but relying on the precedence of a query
            // language we are generating is a needless thing to be right about.
            expression = format!("({expression}) NOT ({excluded})");
        }

        Some(expression)
    }
}

/// Wrap text as an FTS5 phrase, escaping the only character that can end one.
fn phrase(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// Split raw input into chunks, honouring the user's own quotes.
fn chunks(raw: &str) -> Vec<(bool, Chunk)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quoted = false;
    let mut negated = false;

    let mut flush = |current: &mut String, quoted: &mut bool, negated: &mut bool| {
        if !current.is_empty() {
            out.push((
                *negated,
                Chunk {
                    text: std::mem::take(current),
                    quoted: *quoted,
                },
            ));
        }
        *quoted = false;
        *negated = false;
    };

    for c in raw.chars() {
        if in_quote {
            if c == '"' {
                in_quote = false;
            } else {
                current.push(c);
            }
            continue;
        }

        match c {
            '"' => {
                in_quote = true;
                quoted = true;
            }
            // A leading '-' excludes. Anywhere else it is just a character in a
            // word, so `well-known` stays one chunk and becomes the phrase
            // "well known" rather than an exclusion of "known".
            '-' if current.is_empty() && !negated => negated = true,
            c if c.is_whitespace() => flush(&mut current, &mut quoted, &mut negated),
            c => current.push(c),
        }
    }
    // An unterminated quote is treated as if the user had closed it, because
    // the alternative is an error message about syntax they did not know they
    // were writing.
    flush(&mut current, &mut quoted, &mut negated);

    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn fts5(raw: &str) -> Option<String> {
        Query::parse(raw).to_fts5()
    }

    #[test]
    fn words_become_phrases_joined_by_and() {
        assert_eq!(
            fts5("acme holdings").as_deref(),
            Some(r#""acme" AND "holdings""#)
        );
    }

    /// The query an OSINT tool is asked for most, and the one a naive
    /// implementation mangles: `@` and `.` are separators, so this is a
    /// three-token phrase and must be matched in order.
    #[test]
    fn an_email_address_is_one_phrase_not_three_loose_words() {
        assert_eq!(
            fts5("press@acme.example").as_deref(),
            Some(r#""press@acme.example""#)
        );
    }

    #[test]
    fn a_user_quoted_phrase_stays_one_phrase() {
        assert_eq!(
            fts5(r#""acme holdings" ltd"#).as_deref(),
            Some(r#""acme holdings" AND "ltd""#)
        );
    }

    #[test]
    fn a_leading_dash_excludes_and_an_internal_one_does_not() {
        assert_eq!(
            fts5("acme -holdings").as_deref(),
            Some(r#"("acme") NOT ("holdings")"#)
        );
        assert_eq!(fts5("well-known").as_deref(), Some(r#""well-known""#));
    }

    /// A real FTS5 table, because the only opinion that counts about whether
    /// this module emits valid syntax is the parser's.
    ///
    /// Asserting on the shape of the generated string instead would be
    /// checking that the scaffolding matches what this module believes it
    /// emits — which it does by construction, and which says nothing about
    /// whether FTS5 accepts it.
    fn fts5_accepts(expression: &str) -> std::result::Result<(), String> {
        let conn = rusqlite::Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE probe USING fts5(title, body,
                 tokenize = \"unicode61 remove_diacritics 2\");
             INSERT INTO probe VALUES ('Acme Holdings', 'press@acme.example well-known');",
        )
        .map_err(|e| e.to_string())?;

        conn.query_row(
            "SELECT COUNT(*) FROM probe WHERE probe MATCH ?1",
            [expression],
            |r| r.get::<_, i64>(0),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }

    /// Every one of these is a syntax error or a silent change of meaning if
    /// the raw text reaches FTS5. None of them may be either.
    ///
    /// The inputs are not hypothetical: `C++`, `O'Brien` and an unclosed quote
    /// are things people type into search boxes without meaning anything by
    /// them.
    #[test]
    fn hostile_input_never_becomes_syntax() {
        let hostile = [
            r#"unclosed "quote"#,
            "title : secret",
            "body:secret",
            "acme OR *",
            "NEAR(a b, 2)",
            "a AND NOT b",
            "((((",
            "))))",
            "C++",
            "O'Brien",
            r#""" "" """#,
            r#"a" OR "b"#,
            "^anchored",
            "acme*",
            "*",
            "-",
            "---",
            "\\",
            "%_%",
            "{invalid}",
            "a AND",
            "AND",
            "NOT",
            "\u{202e}reversed",
            "emoji 🙂 tail",
            "Müller",
        ];

        for raw in hostile {
            let Some(expression) = fts5(raw) else {
                continue; // Nothing tokenisable: correctly refused before FTS5.
            };
            assert!(
                fts5_accepts(&expression).is_ok(),
                "input {raw:?} produced {expression} which FTS5 rejected: {:?}",
                fts5_accepts(&expression)
            );
        }
    }

    /// The sweep above is a list someone thought of. This is every byte that
    /// could plausibly carry meaning in the query language, tried in the
    /// positions where it would do damage.
    #[test]
    fn no_single_character_can_escape_the_quoting() {
        let dangerous = r#""*():^-+{}[]\/&|!<>=~,.@#$%?'`"#;

        for c in dangerous.chars() {
            for raw in [
                format!("{c}"),
                format!("acme{c}"),
                format!("{c}acme"),
                format!("acme {c} holdings"),
                format!("acme{c}{c}holdings"),
                format!("{c}{c}"),
            ] {
                let Some(expression) = fts5(&raw) else {
                    continue;
                };
                assert!(
                    fts5_accepts(&expression).is_ok(),
                    "input {raw:?} produced {expression} which FTS5 rejected: {:?}",
                    fts5_accepts(&expression)
                );
            }
        }
    }

    /// Quoting must neutralise meaning, not merely survive parsing. An input
    /// that parses but means something else is the worse failure, because it
    /// returns a plausible short list instead of an error.
    #[test]
    fn operators_typed_by_a_user_are_matched_as_words_not_obeyed() {
        // The probe row contains "Acme Holdings" / "press@acme.example
        // well-known". If `OR` were obeyed, this would match it.
        let expression = fts5("acme OR nonexistentword").unwrap();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE probe USING fts5(title, body);
             INSERT INTO probe VALUES ('Acme Holdings', 'press@acme.example');",
        )
        .unwrap();

        let matched: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM probe WHERE probe MATCH ?1",
                [&expression],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            matched, 0,
            "a typed OR was obeyed as an operator: {expression}"
        );

        // And a column filter must not narrow the search to one column.
        let expression = fts5("title : acme").unwrap();
        let matched: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM probe WHERE probe MATCH ?1",
                [&expression],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(matched, 0, "a typed column filter was obeyed: {expression}");
    }

    #[test]
    fn an_inner_quote_is_escaped_rather_than_ending_the_phrase() {
        // The user's own quote characters must not be able to close ours.
        let q = Query {
            positives: vec![Chunk {
                text: r#"a"b"#.into(),
                quoted: false,
            }],
            negatives: vec![],
            prefix_last: false,
        };
        assert_eq!(q.to_fts5().as_deref(), Some(r#""a""b""#));
    }

    #[test]
    fn an_empty_query_matches_nothing_rather_than_everything() {
        assert_eq!(fts5(""), None);
        assert_eq!(fts5("   "), None);
        assert_eq!(fts5("!!! ???"), None);
    }

    /// "Everything except x" is not expressible, and must not silently become
    /// "everything".
    #[test]
    fn a_query_of_only_exclusions_matches_nothing() {
        assert_eq!(fts5("-acme"), None);
        assert!(Query::parse("-acme").is_empty());
    }

    #[test]
    fn typeahead_extends_the_last_word_but_not_a_quoted_phrase() {
        assert_eq!(
            Query::parse_for_typeahead("acme hold").to_fts5().as_deref(),
            Some(r#""acme" AND "hold"*"#)
        );
        assert_eq!(
            Query::parse_for_typeahead(r#"acme "holdings ltd""#)
                .to_fts5()
                .as_deref(),
            Some(r#""acme" AND "holdings ltd""#)
        );
    }
}
