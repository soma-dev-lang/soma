use crate::ast;
use crate::interpreter;
use crate::parser;
use crate::registry::Registry;
use super::lex;

pub fn cmd_repl(_registry: &mut Registry) {
    eprintln!("soma repl v0.1.0 — type expressions to evaluate, :quit to exit");

    let empty_program = ast::Program {
        imports: vec![],
        cells: vec![],
    };
    let mut interp = interpreter::Interpreter::new(&empty_program);

    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        eprint!("soma> ");
        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {}", e);
                break;
            }
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input == ":quit" || input == ":q" || input == "exit" {
            break;
        }

        if input.starts_with("cell ") {
            let tokens = lex(input);
            match parser::Parser::new(tokens).parse_program() {
                Ok(program) => {
                    for cell in &program.cells {
                        interp.register_cell(cell.node.clone());
                        println!("defined cell: {}", cell.node.name);
                    }
                }
                Err(e) => eprintln!("parse error: {}", e),
            }
            continue;
        }

        let wrapper = format!(
            "cell _Repl {{ on _eval() {{ return {} }} }}",
            input
        );

        let tokens = lex(&wrapper);
        match parser::Parser::new(tokens).parse_program() {
            Ok(program) => {
                for cell in &program.cells {
                    interp.register_cell(cell.node.clone());
                }
                match interp.call_signal("_Repl", "_eval", vec![]) {
                    Ok(val) => println!("{}", val),
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Err(e) => eprintln!("parse error: {}", e),
        }
    }
}
