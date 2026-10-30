//! 정의 파일의 요구·제공·파생량 식 (02c §2.3 문법, §2.7 (6)).
//!
//! 수, 이름, `$`매개변수, + - * /, 단항 −, 괄호, min·max·abs·clamp·pick만 받는다.
//! Python 참조 구현(`reaction/ontology/expr.py`)과 같은 값을 내도록 왼쪽 결합으로 평가하고,
//! 단락 평가 없이 모든 인자를 평가하며, 중간값이 유한하지 않으면 오류다.
//! 오류 문자열은 02c의 오류 종류(EXPR_SYNTAX·EXPR_LIMIT·EVAL_*)로 시작한다.

use std::fmt;

/// 식 길이 상한(문자 수)과 괄호·호출 중첩 상한(02c §2.3).
pub const MAX_LEN: usize = 400;
pub const MAX_DEPTH: usize = 16;

/// 함수 이름. 속성·런타임 변수 이름으로 쓸 수 없다.
pub const RESERVED: [&str; 5] = ["min", "max", "abs", "clamp", "pick"];

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(f64),
    Var(String),
    Neg(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Func {
    Min,
    Max,
    Abs,
    Clamp,
    Pick,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    src: String,
    root: Node,
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.src)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    /// 이름 또는 `$`로 시작하는 매개변수(`$` 포함).
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
}

fn syntax(msg: impl fmt::Display) -> String {
    format!("EXPR_SYNTAX: {msg}")
}

/// 토큰: NUMBER `[0-9]+(\.[0-9]+)?`, NAME `[a-z_][a-z0-9_]*`, PARAM `\$[a-z_][a-z0-9_]*(\.[a-z0-9_]+)*`.
fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let head = |c: u8| c.is_ascii_lowercase() || c == b'_';
    let tail = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_';
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            out.push(Tok::Num(src[s..i].parse().map_err(|_| syntax(format!("{s}번째 문자: 수 형식")))?));
            continue;
        }
        if c == b'$' || head(c) {
            let s = i;
            if c == b'$' {
                i += 1;
                if i >= b.len() || !head(b[i]) {
                    return Err(syntax(format!("{s}번째 문자: '$' 뒤에 매개변수 이름이 없음")));
                }
            }
            i += 1;
            while i < b.len() && tail(b[i]) {
                i += 1;
            }
            if c == b'$' {
                while i + 1 < b.len() && b[i] == b'.' && tail(b[i + 1]) {
                    i += 1;
                    while i < b.len() && tail(b[i]) {
                        i += 1;
                    }
                }
            }
            out.push(Tok::Ident(src[s..i].to_string()));
            continue;
        }
        let t = match c {
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b',' => Tok::Comma,
            _ => {
                let ch = src[i..].chars().next().unwrap_or('?');
                return Err(syntax(format!("{i}번째 문자 '{ch}'는 식에 쓸 수 없음")));
            }
        };
        out.push(t);
        i += 1;
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("EXPR_LIMIT: 괄호·호출 중첩이 {MAX_DEPTH}을 넘음"));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Node, String> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Op::Add,
                Some(Tok::Minus) => Op::Sub,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.term()?;
            lhs = Node::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn term(&mut self) -> Result<Node, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Op::Mul,
                Some(Tok::Slash) => Op::Div,
                _ => return Ok(lhs),
            };
            self.pos += 1;
            let rhs = self.unary()?;
            lhs = Node::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        if let Some(Tok::Minus) = self.peek() {
            self.pos += 1;
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Node, String> {
        match self.next() {
            Some(Tok::Num(v)) => Ok(Node::Num(v)),
            Some(Tok::LParen) => {
                self.enter()?;
                let e = self.expr()?;
                self.depth -= 1;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err(syntax("닫는 괄호가 없음")),
                }
            }
            Some(Tok::Ident(name)) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    self.enter()?;
                    let func = match name.as_str() {
                        "min" => Func::Min,
                        "max" => Func::Max,
                        "abs" => Func::Abs,
                        "clamp" => Func::Clamp,
                        "pick" => Func::Pick,
                        other => return Err(syntax(format!("알 수 없는 함수 '{other}'"))),
                    };
                    let mut args = Vec::new();
                    loop {
                        args.push(self.expr()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            _ => return Err(syntax(format!("함수 '{name}'의 인자 목록이 닫히지 않음"))),
                        }
                    }
                    self.depth -= 1;
                    let ok = match func {
                        Func::Min | Func::Max | Func::Pick => args.len() >= 2,
                        Func::Abs => args.len() == 1,
                        Func::Clamp => args.len() == 3,
                    };
                    if !ok {
                        return Err(syntax(format!("함수 '{name}'의 인자 수가 맞지 않음({})", args.len())));
                    }
                    Ok(Node::Call(func, args))
                } else if RESERVED.contains(&name.as_str()) {
                    Err(syntax(format!("'{name}'은 함수 이름이라 변수로 쓸 수 없음")))
                } else {
                    Ok(Node::Var(name))
                }
            }
            Some(t) => Err(syntax(format!("예상하지 못한 기호 {t:?}"))),
            None => Err(syntax("식이 갑자기 끝남")),
        }
    }
}

impl Expr {
    pub fn parse(src: &str) -> Result<Expr, String> {
        if src.chars().count() > MAX_LEN {
            return Err(format!("EXPR_LIMIT: 식 길이가 {MAX_LEN}자를 넘음"));
        }
        let toks = tokenize(src)?;
        if toks.is_empty() {
            return Err(syntax("빈 식"));
        }
        let mut p = Parser { toks, pos: 0, depth: 0 };
        let root = p.expr()?;
        if p.pos != p.toks.len() {
            return Err(syntax(format!("식 뒤에 남은 기호가 있음: '{src}'")));
        }
        Ok(Expr { src: src.to_string(), root })
    }

    pub fn constant(v: f64) -> Expr {
        Expr { src: format!("{v}"), root: Node::Num(v) }
    }

    pub fn source(&self) -> &str {
        &self.src
    }

    /// 식이 쓰는 이름과 `$`매개변수(앞에 `$`가 붙은 채로), 처음 나온 순서.
    pub fn idents(&self) -> Vec<String> {
        fn walk(n: &Node, out: &mut Vec<String>) {
            match n {
                Node::Num(_) => {}
                Node::Var(s) => {
                    if !out.contains(s) {
                        out.push(s.clone());
                    }
                }
                Node::Neg(a) => walk(a, out),
                Node::Bin(_, a, b) => {
                    walk(a, out);
                    walk(b, out);
                }
                Node::Call(_, args) => args.iter().for_each(|a| walk(a, out)),
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    /// `env`는 이름(`$` 매개변수 포함)의 값을 준다. 범주 속성은 values 안의 인덱스, 불리언은 0/1.
    pub fn eval(&self, env: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
        finite(eval_node(&self.root, env)?)
    }
}

fn finite(v: f64) -> Result<f64, String> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("EVAL_NONFINITE: 값 {v}"))
    }
}

fn eval_node(n: &Node, env: &dyn Fn(&str) -> Option<f64>) -> Result<f64, String> {
    match n {
        Node::Num(v) => Ok(*v),
        Node::Var(s) => match env(s) {
            Some(v) => finite(v),
            None => Err(format!("EVAL_NAME: 모르는 이름 '{s}'")),
        },
        Node::Neg(a) => Ok(-eval_node(a, env)?),
        Node::Bin(op, a, b) => {
            let x = eval_node(a, env)?;
            let y = eval_node(b, env)?;
            let v = match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => {
                    if y == 0.0 {
                        return Err("EVAL_DIV0: 0으로 나눔".into());
                    }
                    x / y
                }
            };
            finite(v)
        }
        Node::Call(f, args) => {
            let vals = args.iter().map(|a| eval_node(a, env)).collect::<Result<Vec<f64>, String>>()?;
            let v = match f {
                Func::Min => vals.iter().copied().fold(f64::INFINITY, f64::min),
                Func::Max => vals.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                Func::Abs => vals[0].abs(),
                Func::Clamp => {
                    if vals[1] > vals[2] {
                        return Err(format!("EVAL_CLAMP: 하한 {} > 상한 {}", vals[1], vals[2]));
                    }
                    vals[0].max(vals[1]).min(vals[2])
                }
                Func::Pick => {
                    let i = vals[0];
                    let r = i.round();
                    let n = vals.len() - 1;
                    if (i - r).abs() > 1e-9 || r < 0.0 || r as usize >= n {
                        return Err(format!("EVAL_PICK: 인덱스 {i}가 정수 범위 [0, {n}) 밖"));
                    }
                    vals[r as usize + 1]
                }
            };
            finite(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, vars: &[(&str, f64)]) -> Result<f64, String> {
        let e = Expr::parse(src)?;
        e.eval(&|n| {
            if n == "$fall.ref_down_pct" {
                return Some(20.0);
            }
            if n == "$fall.surface_factor.wet" {
                return Some(1.6);
            }
            vars.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
        })
    }

    #[test]
    fn table() {
        assert_eq!(ev("1 + 2 * 3", &[]).unwrap(), 7.0);
        assert_eq!(ev("(1 + 2) * 3", &[]).unwrap(), 9.0);
        assert_eq!(ev("8 / 4 / 2", &[]).unwrap(), 1.0, "왼쪽 결합");
        assert_eq!(ev("10 - 4 - 3", &[]).unwrap(), 3.0);
        assert_eq!(ev("--2", &[]).unwrap(), 2.0);
        assert_eq!(ev("-x * 2", &[("x", 3.0)]).unwrap(), -6.0);
        assert_eq!(ev("max(grade_pct, 0)", &[("grade_pct", -4.0)]).unwrap(), 0.0);
        assert_eq!(ev("min(3, 1, 2)", &[]).unwrap(), 1.0);
        assert_eq!(ev("abs(0 - 2.5)", &[]).unwrap(), 2.5);
        assert_eq!(ev("clamp(5, 0, 1)", &[]).unwrap(), 1.0);
        assert_eq!(ev("pick(surface, 0, 0.15, 0.4)", &[("surface", 1.0)]).unwrap(), 0.15);
        assert_eq!(ev("$fall.surface_factor.wet * $fall.ref_down_pct", &[]).unwrap(), 32.0);
        let d = ev("clamp(max(0 - grade_pct, 0) / $fall.ref_down_pct * pick(surface, 1.0, $fall.surface_factor.wet, 3.0), 0, 1)", &[("grade_pct", -10.0), ("surface", 1.0)]).unwrap();
        assert!((d - 0.8).abs() < 1e-12);
    }

    #[test]
    fn syntax_errors() {
        for s in ["1.", ".5", "1e3", "max(1)", "clamp(1,2)", "foo(1,2)", "경사", "(1+2", "", "   ", "min()", "max", "a b", "$", "$1a", "$a..b", "$a.", "exp(1)", "lookup(surface, $x)"] {
            let e = Expr::parse(s).err().unwrap_or_else(|| panic!("'{s}'는 오류여야 한다"));
            assert!(e.starts_with("EXPR_SYNTAX"), "{s}: {e}");
        }
        assert!(Expr::parse("$fall.surface_factor.wet").is_ok());
        assert!(Expr::parse("$a.0b").is_ok(), "점 뒤 마디는 숫자로 시작할 수 있다");
        let long = "1+".repeat(200) + "1";
        assert!(Expr::parse(&long).unwrap_err().starts_with("EXPR_LIMIT"));
        let deep = "(".repeat(17) + "1" + &")".repeat(17);
        assert!(Expr::parse(&deep).unwrap_err().starts_with("EXPR_LIMIT"));
        let ok = "(".repeat(16) + "1" + &")".repeat(16);
        assert!(Expr::parse(&ok).is_ok());
    }

    #[test]
    fn eval_errors() {
        assert!(ev("1 / 0", &[]).unwrap_err().starts_with("EVAL_DIV0"));
        assert!(ev("1 / (x - x)", &[("x", 2.0)]).unwrap_err().starts_with("EVAL_DIV0"));
        assert!(ev("y + 1", &[]).unwrap_err().starts_with("EVAL_NAME"));
        assert!(ev("$no.such + 1", &[]).unwrap_err().starts_with("EVAL_NAME"));
        assert!(ev("pick(2, 1, 2)", &[]).unwrap_err().starts_with("EVAL_PICK"), "범위 밖 pick은 오류(02c B3)");
        assert!(ev("pick(0.5, 1, 2)", &[]).unwrap_err().starts_with("EVAL_PICK"));
        assert!(ev("pick(0 - 1, 1, 2)", &[]).unwrap_err().starts_with("EVAL_PICK"));
        assert!(ev("clamp(1, 2, 0)", &[]).unwrap_err().starts_with("EVAL_CLAMP"));
        assert!(ev("min(1, 1 / 0)", &[]).unwrap_err().starts_with("EVAL_DIV0"), "단락 평가 없음");
        assert!(ev("x * 2", &[("x", f64::INFINITY)]).unwrap_err().starts_with("EVAL_NONFINITE"));
    }

    #[test]
    fn idents_in_order() {
        let e = Expr::parse("max(grade_pct, 0) * pick(surface, $a.dry, $a.wet) / $fall.ref_down_pct + grade_pct").unwrap();
        assert_eq!(e.idents(), vec!["grade_pct", "surface", "$a.dry", "$a.wet", "$fall.ref_down_pct"]);
    }
}
