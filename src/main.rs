use ariadne::Source;
use bumpalo::Bump;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use matte::checker::TypeChecker;
use matte::desugar::Desugarer;
use matte::diagnostics::ParserError;
use matte::ir::core::lower;
use matte::ir::lifted::lift;
use matte::lexer::tokenize;
use matte::parser::MatteParser;
use matte::resolve::NameResolver;

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col = before[line_start..].chars().count() + 1;
    (line, col)
}

/// Prints every diagnostic and exits with a failure status.
fn report_all(src: &str, errors: Vec<ParserError<'_>>) -> ! {
    for ParserError(report) in errors {
        report.print(Source::from(src)).unwrap();
    }
    std::process::exit(1);
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a Matte source file into an executable
    Build {
        /// The Matte source file to compile
        input: PathBuf,
        /// The output executable name
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile and run a Matte source file immediately
    Run {
        /// The Matte source file to run
        input: PathBuf,
    },
}

fn compile_to_o(input: &std::path::Path, out_o: &std::path::Path) {
    let src = match std::fs::read_to_string(input) {
        Ok(src) => src,
        Err(err) => {
            eprintln!("{}: {}", input.display(), err);
            std::process::exit(1);
        }
    };

    // Lex.
    let tokens = match tokenize(&src) {
        Ok(tokens) => tokens,
        Err(err) => {
            let (line, col) = line_col(&err.src, err.span.start);
            let bad = &err.src[err.span.clone()];
            let shown = std::fs::canonicalize(input)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| input.display().to_string());
            eprintln!("{shown}:{line}:{col}: error: unexpected `{bad}`");

            let text = err.src.lines().nth(line - 1).unwrap_or("");
            eprintln!("  {text}\n  {}^", " ".repeat(col - 1));
            std::process::exit(1);
        }
    };

    // Everything from here on is allocated in this one arena.
    let arena = Bump::new();

    // Parse.
    let mut parser = MatteParser::new(tokens.into_iter(), &arena);
    let raw_ast = match parser.parse() {
        Ok(ast) => ast,
        Err(err) => report_all(&src, vec![err]),
    };

    // Desugar.
    let ast = match Desugarer::new(&arena).run(raw_ast) {
        Ok(ast) => ast,
        Err(errors) => report_all(&src, errors),
    };

    // Resolve names.
    let resolved = match NameResolver::new(&arena).resolve(&ast) {
        Ok(resolved) => resolved,
        Err(errors) => report_all(&src, errors),
    };

    // Infer types.
    let checked = match TypeChecker::new(&arena).check(&resolved) {
        Ok(checked) => checked,
        Err(errors) => report_all(&src, errors),
    };

    // IR + Codegen
    let core = lower(&arena, &resolved, &checked);
    let lifted = lift(&arena, &core);
    matte::codegen::compile(&lifted, out_o);
}

fn link(o_file: &std::path::Path, out_file: &std::path::Path) {
    let runtime_path = std::path::Path::new("runtime/runtime.c");
    if !runtime_path.exists() {
        eprintln!("Error: runtime/runtime.c not found.");
        std::process::exit(1);
    }

    let status = std::process::Command::new("cc")
        .arg(o_file)
        .arg(runtime_path)
        .arg("-lm")
        .arg("-o")
        .arg(out_file)
        .status()
        .expect("Failed to execute cc");

    if !status.success() {
        eprintln!("Linker failed");
        std::process::exit(1);
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { input, output } => {
            let out_bin = output.unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("");
                if p.as_os_str().is_empty() {
                    PathBuf::from("a.out")
                } else {
                    p
                }
            });
            let mut out_o = out_bin.clone();
            out_o.set_extension("o");

            compile_to_o(&input, &out_o);
            link(&out_o, &out_bin);
            
            // Clean up the intermediate .o file
            let _ = std::fs::remove_file(&out_o);
            
            println!("Compiled {}", out_bin.display());
        }
        Commands::Run { input } => {
            // Put it in /tmp
            let out_o = std::env::temp_dir().join("matte_run_tmp.o");
            let out_bin = std::env::temp_dir().join("matte_run_tmp_bin");

            compile_to_o(&input, &out_o);
            link(&out_o, &out_bin);

            let status = std::process::Command::new(&out_bin)
                .status()
                .expect("Failed to execute generated binary");

            let _ = std::fs::remove_file(&out_o);
            let _ = std::fs::remove_file(&out_bin);

            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }
}
