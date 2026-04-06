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
    // Use Vec to preserve insertion order (HashMap doesn't)
    let mut bindings: Vec<(String, String)> = Vec::new();

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

        // Support bare assignment: a = 1000 → let a = 1000
        let input = if !input.starts_with("let ") && !input.starts_with("cell ")
            && input.contains('=') && !input.contains("==") && !input.contains("!=")
            && !input.contains(">=") && !input.contains("<=")
        {
            let parts: Vec<&str> = input.splitn(2, '=').collect();
            if parts.len() == 2 && parts[0].trim().chars().all(|c| c.is_alphanumeric() || c == '_') {
                format!("let {} = {}", parts[0].trim(), parts[1].trim())
            } else {
                input.to_string()
            }
        } else {
            input.to_string()
        };
        let input = input.as_str();

        // Try parsing as a let statement
        if input.starts_with("let ") {
            // Build let bindings preamble + this new let + return the bound value
            let var_name = input.trim_start_matches("let ")
                .split(|c: char| c == '=' || c.is_whitespace())
                .next()
                .unwrap_or("_")
                .trim()
                .to_string();

            // Store the raw let statement (replace if exists, else append)
            if let Some(pos) = bindings.iter().position(|(n, _)| n == &var_name) {
                bindings[pos].1 = input.to_string();
            } else {
                bindings.push((var_name.clone(), input.to_string()));
            }

            // Build a cell with all bindings in order
            let mut body = String::new();
            for (_, binding) in &bindings {
                body.push_str(binding);
                body.push('\n');
            }
            body.push_str(&format!("return {}", var_name));

            let wrapper = format!("cell _Repl {{ on _eval() {{ {} }} }}", body);
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
            continue;
        }

        // Build expression with all existing let bindings as preamble
        let mut body = String::new();
        for (_, binding) in &bindings {
            body.push_str(binding);
            body.push('\n');
        }
        body.push_str(&format!("return {}", input));

        let wrapper = format!("cell _Repl {{ on _eval() {{ {} }} }}", body);

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
