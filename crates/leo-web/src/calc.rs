use std::collections::HashMap;

pub const MOST_LINES: usize = 400;
const MOST_DEPTH: usize = 64;
const MOST_CALLS: usize = 200_000;

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Op(&'static str),
    Open,
    Close,
    Comma,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let word: String = chars[start..i].iter().collect();
            out.push(Token::Number(
                word.parse()
                    .map_err(|_| format!("“{word}” is not a number"))?,
            ));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let op = match two.as_str() {
            "<=" => Some("<="),
            ">=" => Some(">="),
            "==" => Some("=="),
            "!=" => Some("!="),
            "**" => Some("^"),
            _ => None,
        };
        if let Some(op) = op {
            out.push(Token::Op(op));
            i += 2;
            continue;
        }
        out.push(match c {
            '+' => Token::Op("+"),
            '-' | '−' => Token::Op("-"),
            '*' | '×' | '·' => Token::Op("*"),
            '/' | '÷' => Token::Op("/"),
            '^' => Token::Op("^"),
            '<' => Token::Op("<"),
            '>' => Token::Op(">"),
            '(' | '[' => Token::Open,
            ')' | ']' => Token::Close,
            ',' => Token::Comma,
            'π' => Token::Name("pi".into()),
            _ => return Err(format!("“{c}” is not something the calculator understands")),
        });
        i += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum Expr {
    Number(f64),
    Name(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn op_in(&self, ops: &[&str]) -> Option<&'static str> {
        match self.peek() {
            Some(Token::Op(op)) if ops.contains(op) => Some(op),
            _ => None,
        }
    }

    fn compare(&mut self) -> Result<Expr, String> {
        let mut left = self.sum()?;
        while let Some(op) = self.op_in(&["<", ">", "<=", ">=", "==", "!="]) {
            self.at += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(self.sum()?));
        }
        Ok(left)
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut left = self.product()?;
        while let Some(op) = self.op_in(&["+", "-"]) {
            self.at += 1;
            left = Expr::Binary(op, Box::new(left), Box::new(self.product()?));
        }
        Ok(left)
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            if let Some(op) = self.op_in(&["*", "/"]) {
                self.at += 1;
                left = Expr::Binary(op, Box::new(left), Box::new(self.unary()?));
            } else if matches!(
                self.peek(),
                Some(Token::Number(_) | Token::Name(_) | Token::Open)
            ) {
                left = Expr::Binary("*", Box::new(left), Box::new(self.unary()?));
            } else {
                return Ok(left);
            }
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if let Some(op) = self.op_in(&["-", "+"]) {
            self.at += 1;
            return Ok(Expr::Unary(op, Box::new(self.unary()?)));
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.op_in(&["^"]).is_some() {
            self.at += 1;
            return Ok(Expr::Binary("^", Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Token::Number(n)) => Ok(Expr::Number(n)),
            Some(Token::Name(name)) => {
                if self.peek() == Some(&Token::Open) {
                    self.at += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::Close) {
                        loop {
                            args.push(self.compare()?);
                            match self.next() {
                                Some(Token::Comma) => continue,
                                Some(Token::Close) => break,
                                _ => {
                                    return Err(format!("“{name}(” is missing its closing bracket"))
                                }
                            }
                        }
                    } else {
                        self.at += 1;
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Name(name))
                }
            }
            Some(Token::Open) => {
                let inner = self.compare()?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    _ => Err("a bracket is not closed".into()),
                }
            }
            Some(other) => Err(format!("did not expect {other:?} here")),
            None => Err("the expression ends too early".into()),
        }
    }
}

fn parse(text: &str) -> Result<Expr, String> {
    let mut parser = Parser {
        tokens: tokens(text)?,
        at: 0,
    };
    let expr = parser.compare()?;
    if parser.at < parser.tokens.len() {
        return Err(format!(
            "did not understand the part after token {}",
            parser.at
        ));
    }
    Ok(expr)
}

#[derive(Default)]
pub struct Calculator {
    values: HashMap<String, f64>,
    functions: HashMap<String, (Vec<String>, Expr)>,
    calls: usize,
}

fn built_in(name: &str, args: &[f64]) -> Option<Result<f64, String>> {
    let one = |f: fn(f64) -> f64| {
        if args.len() == 1 {
            Ok(f(args[0]))
        } else {
            Err(format!("{name} takes one number"))
        }
    };
    Some(match name {
        "sin" => one(f64::sin),
        "cos" => one(f64::cos),
        "tan" => one(f64::tan),
        "asin" | "arcsin" => one(f64::asin),
        "acos" | "arccos" => one(f64::acos),
        "atan" | "arctan" => one(f64::atan),
        "sinh" => one(f64::sinh),
        "cosh" => one(f64::cosh),
        "tanh" => one(f64::tanh),
        "exp" => one(f64::exp),
        "ln" => one(f64::ln),
        "log10" => one(f64::log10),
        "log2" => one(f64::log2),
        "sqrt" => one(f64::sqrt),
        "abs" => one(f64::abs),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "round" => one(f64::round),
        "sign" => one(f64::signum),
        "log" if args.len() == 2 => Ok(args[1].ln() / args[0].ln()),
        "log" => one(f64::ln),
        "atan2" if args.len() == 2 => Ok(args[0].atan2(args[1])),
        "min" if !args.is_empty() => Ok(args.iter().copied().fold(f64::INFINITY, f64::min)),
        "max" if !args.is_empty() => Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "sum" => Ok(args.iter().sum()),
        "mean" if !args.is_empty() => Ok(args.iter().sum::<f64>() / args.len() as f64),
        "if" if args.len() == 3 => Ok(if args[0] != 0.0 { args[1] } else { args[2] }),
        "if" => Err("if takes three parts: if(condition, then, otherwise)".into()),
        _ => return None,
    })
}

impl Calculator {
    fn value_of(
        &mut self,
        expr: &Expr,
        scope: &HashMap<String, f64>,
        depth: usize,
    ) -> Result<f64, String> {
        if depth > MOST_DEPTH {
            return Err("functions call each other too deeply".into());
        }
        self.calls += 1;
        if self.calls > MOST_CALLS {
            return Err("this is too much work for one calculation".into());
        }
        Ok(match expr {
            Expr::Number(n) => *n,
            Expr::Name(name) => match scope.get(name).or_else(|| self.values.get(name)) {
                Some(v) => *v,
                None => match name.as_str() {
                    "pi" | "π" => std::f64::consts::PI,
                    "e" => std::f64::consts::E,
                    "phi" => (1.0 + 5f64.sqrt()) / 2.0,
                    "inf" => f64::INFINITY,
                    _ => return Err(format!("“{name}” has no value yet")),
                },
            },
            Expr::Unary(op, inner) => {
                let v = self.value_of(inner, scope, depth + 1)?;
                if *op == "-" {
                    -v
                } else {
                    v
                }
            }
            Expr::Binary(op, a, b) => {
                if let (&"^", Expr::Name(base)) = (op, a.as_ref()) {
                    if base == "e" && !scope.contains_key("e") && !self.values.contains_key("e") {
                        return Ok(self.value_of(b, scope, depth + 1)?.exp());
                    }
                }
                let x = self.value_of(a, scope, depth + 1)?;
                let y = self.value_of(b, scope, depth + 1)?;
                let truth = |t: bool| if t { 1.0 } else { 0.0 };
                match *op {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => x / y,
                    "^" => x.powf(y),
                    "<" => truth(x < y),
                    ">" => truth(x > y),
                    "<=" => truth(x <= y),
                    ">=" => truth(x >= y),
                    "==" => truth((x - y).abs() <= 1e-12 * x.abs().max(y.abs()).max(1.0)),
                    "!=" => truth((x - y).abs() > 1e-12 * x.abs().max(y.abs()).max(1.0)),
                    _ => return Err(format!("unknown operation {op}")),
                }
            }
            Expr::Call(name, args) => {
                if name == "if" && args.len() == 3 {
                    let test = self.value_of(&args[0], scope, depth + 1)?;
                    return self.value_of(
                        if test != 0.0 { &args[1] } else { &args[2] },
                        scope,
                        depth + 1,
                    );
                }
                let given = args
                    .iter()
                    .map(|a| self.value_of(a, scope, depth + 1))
                    .collect::<Result<Vec<f64>, String>>()?;
                if let Some((params, body)) = self.functions.get(name).cloned() {
                    if params.len() != given.len() {
                        return Err(format!("{name} takes {} number(s)", params.len()));
                    }
                    let inner: HashMap<String, f64> = params.into_iter().zip(given).collect();
                    return self.value_of(&body, &inner, depth + 1);
                }
                match built_in(name, &given) {
                    Some(result) => result?,
                    None => return Err(format!("there is no function called “{name}”")),
                }
            }
        })
    }

    pub fn line(&mut self, line: &str) -> Result<String, String> {
        let text = line.trim().trim_end_matches(';').trim();
        if let Some((left, right)) = split_definition(text) {
            let left = left.trim();
            if let Some(open) = left.find('(').filter(|_| left.ends_with(')')) {
                let name = left[..open].trim().to_string();
                let params: Vec<String> = left[open + 1..left.len() - 1]
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                if !valid_name(&name) || params.iter().any(|p| !valid_name(p)) {
                    return Err(format!("“{left}” is not a function name with parameters"));
                }
                let body = parse(right)?;
                self.functions.insert(name.clone(), (params.clone(), body));
                return Ok(format!("{name}({}) = {}", params.join(", "), right.trim()));
            }
            if !valid_name(left) {
                return Err(format!("“{left}” cannot be given a value"));
            }
            self.calls = 0;
            let value = self.value_of(&parse(right)?, &HashMap::new(), 0)?;
            self.values.insert(left.to_string(), value);
            return Ok(format!("{left} = {}", shown(value)));
        }
        self.calls = 0;
        let value = self.value_of(&parse(text)?, &HashMap::new(), 0)?;
        Ok(format!("{text} = {}", shown(value)))
    }
}

fn split_definition(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'=' {
            let before = i.checked_sub(1).map(|j| bytes[j]);
            let after = bytes.get(i + 1).copied();
            if !matches!(before, Some(b'<' | b'>' | b'!' | b'=')) && after != Some(b'=') {
                return Some((&text[..i], &text[i + 1..]));
            }
        }
    }
    None
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && built_in(name, &[0.0]).is_none()
        && !["pi", "e"].contains(&name)
}

pub fn shown(value: f64) -> String {
    if value.is_nan() {
        return "not a number".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "infinity" } else { "-infinity" }.into();
    }
    if value != 0.0 && (value.abs() >= 1e12 || value.abs() < 1e-6) {
        return format!("{value:.10e}");
    }
    let text = format!("{value:.10}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.to_string()
    }
}

pub fn run(steps: &str) -> String {
    let mut calc = Calculator::default();
    let mut out = Vec::new();
    for (n, line) in steps
        .lines()
        .flat_map(|l| l.split(';'))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .enumerate()
    {
        if n >= MOST_LINES {
            out.push(format!("(stopped after {MOST_LINES} lines)"));
            break;
        }
        out.push(match calc.line(line) {
            Ok(text) => text,
            Err(why) => format!("{}  →  That did not work: {why}", line.trim()),
        });
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_golden_section_step_is_computed_exactly() {
        let said = run("f(x) = x^2 + 4*cos(x)\nr = (3 - sqrt(5))/2\nL = 1; U = 2\na = L + r*(U - L)\nb = U - r*(U - L)\nf(a)\nf(b)\nL = if(f(a) > f(b), a, L)");
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines[0], "f(x) = x^2 + 4*cos(x)");
        assert_eq!(lines[1], "r = 0.3819660113");
        assert_eq!(lines[4], "a = 1.3819660113");
        assert!(lines[6].starts_with("f(a) = 2.660"), "{said}");
        assert!(lines[7].starts_with("f(b) = 2.429"), "{said}");
        assert_eq!(lines[8], "L = 1.3819660113");
    }

    #[test]
    fn newtons_method_diverging_is_shown_as_it_happens() {
        let said = run("d(x) = 2x - 4 sin(x)\ndd(x) = 2 - 4cos(x)\nx = 1\nx = x - d(x)/dd(x)\nx = x - d(x)/dd(x)");
        let lines: Vec<&str> = said.lines().collect();
        assert!(lines[3].starts_with("x = -7.4727"), "{said}");
        assert!(lines[4].starts_with("x = 14.478"), "{said}");
    }

    #[test]
    fn mistakes_are_explained_and_the_rest_still_runs() {
        let said =
            run("y = q + 1\nz = 2^10\nhack(\nfoo(3)\nloop(x) = loop(x)\nloop(1)\n1/0\nsqrt(-1)");
        let lines: Vec<&str> = said.lines().collect();
        assert!(lines[0].contains("That did not work: “q” has no value yet"));
        assert_eq!(lines[1], "z = 1024");
        assert!(lines[2].contains("That did not work"));
        assert!(lines[3].contains("no function called “foo”"));
        assert!(lines[5].contains("too deeply"));
        assert_eq!(lines[6], "1/0 = infinity");
        assert_eq!(lines[7], "sqrt(-1) = not a number");
    }

    #[test]
    fn familiar_ways_of_writing_math_are_read() {
        assert_eq!(run("2 ** 3"), "2 ** 3 = 8");
        assert_eq!(run("e^1"), "e^1 = 2.7182818285");
        assert_eq!(run("3(2 + 1)"), "3(2 + 1) = 9");
        assert_eq!(run("-2^2"), "-2^2 = -4");
        assert_eq!(run("2^-1"), "2^-1 = 0.5");
        assert_eq!(run("1.5e3 + 1"), "1.5e3 + 1 = 1501");
        assert_eq!(run("log(2, 8)"), "log(2, 8) = 3");
        assert_eq!(
            run("max(1, 7, 3) - min(4, 2)"),
            "max(1, 7, 3) - min(4, 2) = 5"
        );
        assert_eq!(run("1e-9"), "1e-9 = 1.0000000000e-9");
        assert_eq!(run("# a comment\n2π"), "2π = 6.2831853072");
        assert!(run("sin = 3").contains("cannot be given a value"));
    }

    #[test]
    fn runaway_work_is_stopped() {
        let many = "x = 1\n".repeat(MOST_LINES + 5);
        assert!(run(&many).ends_with(&format!("(stopped after {MOST_LINES} lines)")));
        let said = run("f(x) = if(x > 0, f(x - 1) + f(x - 1), 1)\nf(40)");
        assert!(said.contains("That did not work"), "{said}");
    }
}
