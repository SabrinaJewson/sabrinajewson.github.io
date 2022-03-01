//! Simple template system.

use ::std::{
    error::Error,
    fmt::{self, Display, Formatter},
    ops::Range,
};

pub(crate) struct Template {
    origin: String,
    substitutions: Vec<Range<usize>>,
}

impl Template {
    pub(crate) fn new(origin: String) -> Result<Self, ParseError> {
        let mut substitutions = Vec::new();

        let mut bytes = origin.as_bytes();
        while let Some(start) = memchr::memchr(b'\\', bytes) {
            let end = match bytes
                .get(start + 1)
                .ok_or(ParseErrorKind::TrailingBackslash)?
            {
                b'\\' => start + 2,
                b'{' => memchr::memchr(b'}', bytes).ok_or(ParseErrorKind::NoClosingBrace)? + 1,
                &c => return Err(ParseErrorKind::UnexpectedAfterBackslash(char::from(c)).into()),
            };

            bytes = &bytes[end..];
            substitutions.push(start..end);
        }

        Ok(Self {
            origin,
            substitutions,
        })
    }

    pub(crate) fn apply<const VARS_LENGTH: usize>(
        &self,
        output: &mut String,
        vars: [(&str, &str); VARS_LENGTH],
    ) {
        let mut rest = &*self.origin;
        for substitution in &self.substitutions {
            output.push_str(&rest[..substitution.start]);
            match rest.as_bytes()[substitution.start + 1] {
                b'\\' => output.push('\\'),
                b'{' => {
                    let var_name = &rest[substitution.start + 2..substitution.end - 1];
                    if let Some((_, value)) = vars.iter().find(|&&(k, _)| k == var_name) {
                        output.push_str(value);
                    } else {
                        // If the variable is not found, don't substitute it.
                        output.push_str(&rest[substitution.clone()]);
                    }
                }
                _ => unreachable!(),
            }
            rest = &rest[substitution.end..];
        }
        output.push_str(rest);
    }
}

#[derive(Debug)]
pub(crate) struct ParseError {
    kind: ParseErrorKind,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("failed to parse template")
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.kind)
    }
}

#[derive(Debug)]
enum ParseErrorKind {
    TrailingBackslash,
    NoClosingBrace,
    UnexpectedAfterBackslash(char),
}

impl Display for ParseErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrailingBackslash => f.write_str("trailing backslash"),
            Self::NoClosingBrace => f.write_str("no closing `}`"),
            Self::UnexpectedAfterBackslash(c) => {
                write!(f, "unexpected character `{c}` after backslash")
            }
        }
    }
}

impl Error for ParseErrorKind {}

impl From<ParseErrorKind> for ParseError {
    fn from(kind: ParseErrorKind) -> Self {
        ParseError { kind }
    }
}

#[cfg(test)]
mod tests {
    use super::Template;

    #[track_caller]
    fn template<const VARS_LENGTH: usize>(src: &str, vars: [(&str, &str); VARS_LENGTH]) -> String {
        let mut output = String::new();
        let template = Template::new(src.to_owned()).unwrap();
        template.apply(&mut output, vars);
        output
    }

    #[test]
    fn basic() {
        assert_eq!(template(r"", []), r"");
        assert_eq!(template(r"simple", []), r"simple");
        assert_eq!(template(r"foo\\", []), r"foo\");
        assert_eq!(template(r"foo\\bar", []), r"foo\bar");
        assert_eq!(template(r"\\bar", []), r"\bar");
        assert_eq!(
            template(
                r"\{best programming lang}",
                [("best programming lang", "rust")]
            ),
            r"rust"
        );
        assert_eq!(
            template(
                r":\{best programming lang}:",
                [("best programming lang", "rust")]
            ),
            r":rust:"
        );
        assert_eq!(
            template(r"\{1} text here \{2}", [("1", "one"), ("2", "two")]),
            r"one text here two"
        );
        assert_eq!(
            template(r"(\{variable not present})", []),
            r"(\{variable not present})"
        );
    }
}
