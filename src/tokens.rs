//! Cheap token estimator. Not a real tokenizer: alphanumeric runs cost one token
//! per ~4 chars, every punctuation char and newline costs one, plain spaces are
//! free (they merge into the next word). That mirrors why JSON is so expensive.

pub fn estimate(s: &str) -> usize {
    let mut tokens = 0usize;
    let mut run = 0usize;
    for c in s.chars() {
        if c.is_alphanumeric() {
            run += 1;
            continue;
        }
        if run > 0 {
            tokens += run.div_ceil(4);
            run = 0;
        }
        if !c.is_whitespace() || c == '\n' {
            tokens += 1;
        }
    }
    if run > 0 {
        tokens += run.div_ceil(4);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_costs_more_than_prose_of_same_length() {
        let json = r#"{"a":1,"b":2,"c":3}"#;
        let prose = "abcd efgh ijkl mnop";
        assert!(estimate(json) > estimate(prose));
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate(""), 0);
    }
}
