use proptest::prelude::*;
use wat_core::lexer::{lex, Token, LITERAL_MARK};

/// Generate safe shell words: printable ASCII minus shell metacharacters and quotes.
fn safe_word() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9._/:-]{1,20}".prop_map(|s| s)
}

proptest! {
    /// Lex a sequence of safe words separated by spaces; the token stream should contain exactly
    /// those words (no operators, no merging) and end with Eof.
    #[test]
    fn lex_safe_words_round_trip(words in prop::collection::vec(safe_word(), 1..=8)) {
        let input = words.join(" ");
        let tokens = lex(&input).unwrap();
        let word_tokens: Vec<_> = tokens.iter()
            .filter_map(|s| if let Token::Word(w) = &s.token { Some(w.as_str()) } else { None })
            .collect();
        prop_assert_eq!(word_tokens, words.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    /// Lex a safe word: token stream is [Word(w), Eof] and the word value is preserved.
    #[test]
    fn lex_single_safe_word(w in safe_word()) {
        let tokens = lex(&w).unwrap();
        prop_assert_eq!(tokens.len(), 2);
        prop_assert_eq!(&tokens[0].token, &Token::Word(w.clone()));
        prop_assert_eq!(&tokens[1].token, &Token::Eof);
    }

    /// Single-quoting any string with no single-quote yields exactly one word
    /// whose content is the original string, bracketed by the internal literal
    /// markers (which the expander later strips). Slicing off one marker at each
    /// end recovers the input verbatim — even if it contains marker characters.
    #[test]
    fn single_quote_identity(s in "[^']{0,30}") {
        let input = format!("'{}'", s);
        let tokens = lex(&input).unwrap();
        let words: Vec<&str> = tokens.iter()
            .filter_map(|t| if let Token::Word(w) = &t.token { Some(w.as_str()) } else { None })
            .collect();
        prop_assert_eq!(words.len(), 1);
        let mut chars = words[0].chars();
        prop_assert_eq!(chars.next(), Some(LITERAL_MARK));
        prop_assert_eq!(chars.next_back(), Some(LITERAL_MARK));
        let inner: String = chars.collect();
        prop_assert_eq!(inner, s);
    }
}
