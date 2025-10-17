use crate::utils::Text;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LineEnd {
    /// Line Feed (LF) - Common on Unix, Linux, and macOS (`\n`).
    Lf,
    /// Carriage Return + Line Feed (CRLF) - Used on Windows (`\r\n`).
    CrLf,
}

impl From<LineEnd> for &str {
    fn from(value: LineEnd) -> Self {
        match value {
            LineEnd::Lf => "\n",
            LineEnd::CrLf => "\r\n",
        }
    }
}

impl From<LineEnd> for &[u8] {
    fn from(value: LineEnd) -> Self {
        match value {
            LineEnd::Lf => b"\n",
            LineEnd::CrLf => b"\r\n",
        }
    }
}

impl LineEnd {
    /// Strip only line ending from `line`. Returns pair of stripped line and optionally stripped line ending.
    ///
    /// Assumes that if line has line ending, then it is last chars.
    pub fn strip<T: ?Sized + Text + ToOwned>(line: &T) -> (&T, Option<LineEnd>) {
        let mut line_ending = None;
        let line_without_lf = line.strip_suffix("\n").inspect(|_| {
            line_ending = Some(LineEnd::Lf);
        });
        let line_without_crlf = line_without_lf
            .and_then(|line| line.strip_suffix("\r"))
            .inspect(|_| {
                line_ending = Some(LineEnd::CrLf);
            });
        let stripped_line = line_without_crlf.or(line_without_lf);

        (stripped_line.unwrap_or(line), line_ending)
    }

    /// Choose line ending based on the scores.
    pub fn choose_from_scores(lf_score: usize, crlf_score: usize) -> LineEnd {
        #[allow(clippy::if_same_then_else)]
        if lf_score > crlf_score {
            LineEnd::Lf
        } else if lf_score < crlf_score {
            LineEnd::CrLf
        } else if cfg!(windows) {
            LineEnd::CrLf
        } else {
            LineEnd::Lf
        }
    }

    /// Returns most common line ending.
    pub fn most_common<T: ?Sized + Text + ToOwned>(input: &T) -> LineEnd {
        let mut lf_score: usize = 0;
        let mut crlf_score: usize = 0;

        let mut previous_is_cr = false;
        for byte in input.as_bytes() {
            match byte {
                b'\r' => {
                    previous_is_cr = true;
                }
                b'\n' => {
                    if previous_is_cr {
                        crlf_score += 1;
                    } else {
                        lf_score += 1;
                    }
                    previous_is_cr = false;
                }
                _ => {
                    previous_is_cr = false;
                    continue;
                }
            }
        }

        LineEnd::choose_from_scores(lf_score, crlf_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("hello")]
    #[case("\r")]
    #[case("")]
    #[case("\rhello")]
    #[case("hello \r")]
    #[case("\r\nhello")]
    #[case("\nhello")]
    #[case("hello\n ")]
    #[case("hello\r\n ")]
    fn strip_no_line_ending(#[case] input: &str) {
        let stripped = LineEnd::strip(input);
        assert_eq!((input, None), stripped);
    }

    #[rstest]
    #[case("hello\n")]
    #[case("hello\r\n")]
    #[case("hello \n")]
    #[case("hello \r\n")]
    #[case("\r\nhello \n")]
    #[case("hello \r\n")]
    fn strip_line_ending(#[case] input: &str) {
        let (stripped, line_ending) = LineEnd::strip(input);
        assert!(
            input.len().saturating_sub(2) <= stripped.len()
                && stripped.len() < input.len()
                && line_ending.is_some(),
            "Expected no newline at the end, but got: {:#?}\nOriginal line is: {:#?}",
            stripped,
            input
        );
    }

    #[rstest]
    #[case("\n\r\n")]
    #[case("")]
    #[case("\r")]
    #[case("\r\n\n")]
    #[case("\r\n\r\n\n\n")]
    #[case("\r\n \r\n\n\n")]
    fn most_common_if_eq(#[case] input: &str) {
        let most_common = LineEnd::most_common(input);
        assert_eq!(
            most_common,
            if cfg!(windows) {
                LineEnd::CrLf
            } else {
                LineEnd::Lf
            }
        );
    }

    #[rstest]
    #[case("\n\r")]
    #[case("\r\n\n\n")]
    #[case("\n\n\r\n")]
    #[case(" \n\n  \r\n ")]
    #[case("\r \n")]
    #[case("\r\n\n\n\n")]
    fn most_common_if_neq_lf(#[case] input: &str) {
        let most_common = LineEnd::most_common(input);
        assert_eq!(most_common, LineEnd::Lf);
    }

    #[rstest]
    #[case("\r\n")]
    #[case("\r\n\r\n")]
    #[case("\r\n\r\n\n")]
    #[case("\n\r\n\r\n")]
    fn most_common_if_neq_crlf(#[case] input: &str) {
        let most_common = LineEnd::most_common(input);
        assert_eq!(most_common, LineEnd::CrLf);
    }
}
