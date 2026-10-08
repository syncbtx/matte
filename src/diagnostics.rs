use crate::lexer::{Token, TokenKind};
use ariadne::{Color, Label, Report, ReportKind};
use std::ops::Range;
use vpratt::VprattError;

#[derive(Debug)]
pub struct ParserError<'a>(pub Report<'a, Range<usize>>);

impl<'a> From<VprattError<Token<'a>, TokenKind<'a>>> for ParserError<'a> {
    fn from(err: VprattError<Token<'a>, TokenKind<'a>>) -> Self {
        ParserError(match err {
            VprattError::UnexpectedEOF => Report::build(ReportKind::Error, 0..0)
                .with_message("unexpected end of file")
                .with_help("the expression appears to be incomplete")
                .finish(),

            VprattError::UnexpectedToken(token) => {
                Report::build(ReportKind::Error, token.span.clone())
                    .with_message(format!("unexpected token `{:?}`", token.kind))
                    .with_label(
                        Label::new(token.span)
                            .with_message("not valid here")
                            .with_color(Color::Red),
                    )
                    .finish()
            }

            VprattError::UnmatchedDelimiter(expected, token) => {
                Report::build(ReportKind::Error, token.span.clone())
                    .with_message("mismatched delimiter")
                    .with_label(
                        Label::new(token.span)
                            .with_message(format!("expected `{:?}` to close here", expected))
                            .with_color(Color::Red),
                    )
                    .finish()
            }

            VprattError::ExpectedTokenMismatch(expected, token) => {
                Report::build(ReportKind::Error, token.span.clone())
                    .with_message(format!("expected `{:?}`", expected))
                    .with_label(
                        Label::new(token.span)
                            .with_message(format!(
                                "found `{:?}` here, expected `{:?}`",
                                token.kind, expected
                            ))
                            .with_color(Color::Yellow),
                    )
                    .finish()
            }
        })
    }
}
