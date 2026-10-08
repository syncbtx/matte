use ariadne::Source;
use bumpalo::Bump;

use matte::lexer::tokenize;
use matte::parser::MatteParser;
use matte::desugar::Desugarer;
use matte::diagnostics::ParserError;

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}

fn main() {
    let argv = std::env::args().skip(1).collect::<Vec<_>>();
    if argv.is_empty() {
        eprintln!("Usage: matte <file>");
        std::process::exit(1);
    }
    let path = argv[0].clone();
    let src = std::fs::read_to_string(&path).unwrap();

    match tokenize(&src) {
        Ok(tokens) => {
            let arena = Bump::new();
            let mut parser = MatteParser::new(tokens.into_iter(), &arena);

            match parser.parse() {
                Ok(raw_ast) => {
                    let desugarer = Desugarer::new(&arena);
                    match desugarer.desugar(&raw_ast) {
                        Ok(core_ast) => {
                            println!("{:#?}", core_ast);
                        }
                        Err(ParserError(report)) => {
                            report.print(Source::from(src.as_str())).unwrap();
                        }
                    }
                }
                Err(ParserError(report)) => {
                    report.print(Source::from(src.as_str())).unwrap();
                }
            }
        }
        Err(err) => {
            let (line, col) = line_col(&err.src, err.span.start);
            let bad = &err.src[err.span.clone()];
            let shown = std::fs::canonicalize(path.clone())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.to_string());
            eprintln!("{shown}:{line}:{col}: error: unexpected `{bad}`");

            let text = err.src.lines().nth(line - 1).unwrap_or("");
            eprintln!("  {text}\n  {}^", " ".repeat(col - 1));
            std::process::exit(1);
        }
    }
}
