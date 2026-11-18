use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum MathError {
    Parse(String),
    Eval(String),
    Unsupported(String),
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MathError::Parse(s) => write!(f, "parse error: {s}"),
            MathError::Eval(s) => write!(f, "cannot evaluate: {s}"),
            MathError::Unsupported(s) => write!(f, "unsupported: {s}"),
        }
    }
}
impl std::error::Error for MathError {}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(String),
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Func(String, Box<Expr>),
}

use Expr::*;

const FUNCS: &[&str] = &["sin", "cos", "tan", "asin", "acos", "atan", "exp", "ln", "log", "sqrt", "abs"];
const MAX_LEN: usize = 1000;
const MAX_DEPTH: usize = 100;

// ------------------------------------------------------------------ lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

fn lex(s: &str) -> Result<Vec<Tok>, MathError> {
    if s.len() > MAX_LEN {
        return Err(MathError::Parse(format!("expression longer than {MAX_LEN} characters")));
    }
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            // exponent: 1e-3
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                let mut j = i + 1;
                if j < c.len() && (c[j] == '+' || c[j] == '-') {
                    j += 1;
                }
                if j < c.len() && c[j].is_ascii_digit() {
                    while j < c.len() && c[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let t: String = c[start..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| MathError::Parse(format!("bad number `{t}`")))?));
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(c[start..i].iter().collect()));
        } else if "+-*/^".contains(ch) {
            out.push(Tok::Op(ch));
            i += 1;
        } else if ch == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if ch == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else {
            return Err(MathError::Parse(format!("unexpected character `{ch}`")));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ parser

struct Parser {
    t: Vec<Tok>,
    i: usize,
    depth: usize,
}

/// Parse an expression. Precedence: `^` (right-assoc) > unary `-` > `* /` and implicit
/// multiplication > `+ -`. So `-2^2 = -4` and `2^3^2 = 512`.
pub fn parse(s: &str) -> Result<Expr, MathError> {
    let mut p = Parser { t: lex(s)?, i: 0, depth: 0 };
    if p.t.is_empty() {
        return Err(MathError::Parse("empty expression".into()));
    }
    let e = p.sum()?;
    if p.i != p.t.len() {
        return Err(MathError::Parse(format!("unexpected `{}`", p.describe())));
    }
    Ok(e)
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn describe(&self) -> String {
        match self.peek() {
            Some(Tok::Num(n)) => n.to_string(),
            Some(Tok::Ident(s)) => s.clone(),
            Some(Tok::Op(c)) => c.to_string(),
            Some(Tok::LParen) => "(".into(),
            Some(Tok::RParen) => ")".into(),
            None => "end of input".into(),
        }
    }
    fn enter(&mut self) -> Result<(), MathError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(MathError::Parse("expression nested too deeply".into()))
        } else {
            Ok(())
        }
    }

    fn sum(&mut self) -> Result<Expr, MathError> {
        let mut l = self.product()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.product()?;
            l = if c == '+' { Add(l.into(), r.into()) } else { Sub(l.into(), r.into()) };
        }
        Ok(l)
    }

    fn product(&mut self) -> Result<Expr, MathError> {
        let mut l = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(c @ ('*' | '/'))) => {
                    self.i += 1;
                    let r = self.unary()?;
                    l = if c == '*' { Mul(l.into(), r.into()) } else { Div(l.into(), r.into()) };
                }
                // implicit multiplication: `2x`, `2(x+1)`, `(a)(b)`, `x sin(x)`
                Some(Tok::Num(_)) | Some(Tok::Ident(_)) | Some(Tok::LParen) => {
                    let r = self.unary()?;
                    l = Mul(l.into(), r.into());
                }
                _ => return Ok(l),
            }
        }
    }

    fn unary(&mut self) -> Result<Expr, MathError> {
        self.enter()?;
        let r = if let Some(Tok::Op(c @ ('-' | '+'))) = self.peek().cloned() {
            self.i += 1;
            let e = self.unary()?;
            if c == '-' { Ok(Neg(e.into())) } else { Ok(e) }
        } else {
            self.power()
        };
        self.depth -= 1;
        r
    }

    fn power(&mut self) -> Result<Expr, MathError> {
        let base = self.atom()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.i += 1;
            let exp = self.unary()?; // right associative, allows 2^-1
            return Ok(Pow(base.into(), exp.into()));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, MathError> {
        self.enter()?;
        let r = match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(Num(n))
            }
            Some(Tok::Ident(name)) => {
                self.i += 1;
                if FUNCS.contains(&name.as_str()) {
                    if self.peek() != Some(&Tok::LParen) {
                        return Err(MathError::Parse(format!("`{name}` needs an argument in parentheses")));
                    }
                    self.i += 1;
                    let arg = self.sum()?;
                    self.close()?;
                    Ok(Func(name, arg.into()))
                } else {
                    Ok(match name.as_str() {
                        "pi" => Num(std::f64::consts::PI),
                        "e" => Num(std::f64::consts::E),
                        _ => Var(name),
                    })
                }
            }
            Some(Tok::LParen) => {
                self.i += 1;
                let e = self.sum()?;
                self.close()?;
                Ok(e)
            }
            _ => Err(MathError::Parse(format!("unexpected `{}`", self.describe()))),
        };
        self.depth -= 1;
        r
    }

    fn close(&mut self) -> Result<(), MathError> {
        if self.peek() == Some(&Tok::RParen) {
            self.i += 1;
            Ok(())
        } else {
            Err(MathError::Parse("missing `)`".into()))
        }
    }
}

// ------------------------------------------------------------------ evaluation

impl Expr {
    pub fn eval(&self, vars: &HashMap<String, f64>) -> Result<f64, MathError> {
        let v = match self {
            Num(n) => *n,
            Var(x) => *vars.get(x).ok_or_else(|| MathError::Eval(format!("variable `{x}` has no value")))?,
            Neg(a) => -a.eval(vars)?,
            Add(a, b) => a.eval(vars)? + b.eval(vars)?,
            Sub(a, b) => a.eval(vars)? - b.eval(vars)?,
            Mul(a, b) => a.eval(vars)? * b.eval(vars)?,
            Div(a, b) => {
                let d = b.eval(vars)?;
                if d == 0.0 {
                    return Err(MathError::Eval("division by zero".into()));
                }
                a.eval(vars)? / d
            }
            Pow(a, b) => a.eval(vars)?.powf(b.eval(vars)?),
            Func(f, a) => {
                let x = a.eval(vars)?;
                match f.as_str() {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "asin" => x.asin(),
                    "acos" => x.acos(),
                    "atan" => x.atan(),
                    "exp" => x.exp(),
                    "ln" => x.ln(),
                    "log" => x.log10(),
                    "sqrt" => x.sqrt(),
                    "abs" => x.abs(),
                    _ => return Err(MathError::Eval(format!("unknown function `{f}`"))),
                }
            }
        };
        if v.is_nan() {
            Err(MathError::Eval("result is not a number (outside the function's domain)".into()))
        } else if v.is_infinite() {
            Err(MathError::Eval("result is infinite".into()))
        } else {
            Ok(v)
        }
    }

    /// Evaluate with a single variable.
    pub fn eval_at(&self, var: &str, x: f64) -> Result<f64, MathError> {
        let mut m = HashMap::new();
        m.insert(var.to_string(), x);
        self.eval(&m)
    }

    fn contains(&self, var: &str) -> bool {
        match self {
            Num(_) => false,
            Var(x) => x == var,
            Neg(a) | Func(_, a) => a.contains(var),
            Add(a, b) | Sub(a, b) | Mul(a, b) | Div(a, b) | Pow(a, b) => a.contains(var) || b.contains(var),
        }
    }

    /// Symbolic derivative with respect to `var`, simplified.
    pub fn derivative(&self, var: &str) -> Result<Expr, MathError> {
        Ok(self.d(var)?.simplify())
    }

    fn d(&self, v: &str) -> Result<Expr, MathError> {
        let b = |e: Expr| Box::new(e);
        Ok(match self {
            Num(_) => Num(0.0),
            Var(x) => Num(if x == v { 1.0 } else { 0.0 }),
            Neg(a) => Neg(b(a.d(v)?)),
            Add(a, c) => Add(b(a.d(v)?), b(c.d(v)?)),
            Sub(a, c) => Sub(b(a.d(v)?), b(c.d(v)?)),
            Mul(a, c) => Add(b(Mul(b(a.d(v)?), c.clone())), b(Mul(a.clone(), b(c.d(v)?)))),
            Div(a, c) => Div(
                b(Sub(b(Mul(b(a.d(v)?), c.clone())), b(Mul(a.clone(), b(c.d(v)?))))),
                b(Pow(c.clone(), b(Num(2.0)))),
            ),
            Pow(base, exp) => {
                if !exp.contains(v) {
                    // (f^n)' = n f^(n-1) f'
                    Mul(b(Mul(exp.clone(), b(Pow(base.clone(), b(Sub(exp.clone(), b(Num(1.0)))))))), b(base.d(v)?))
                } else if !base.contains(v) {
                    // (a^g)' = a^g ln(a) g'
                    Mul(b(Mul(b(self.clone()), b(Func("ln".into(), base.clone())))), b(exp.d(v)?))
                } else {
                    // (f^g)' = f^g (g' ln f + g f'/f)
                    Mul(
                        b(self.clone()),
                        b(Add(b(Mul(b(exp.d(v)?), b(Func("ln".into(), base.clone())))), b(Div(b(Mul(exp.clone(), b(base.d(v)?))), base.clone())))),
                    )
                }
            }
            Func(f, a) => {
                let inner = a.d(v)?;
                let outer = match f.as_str() {
                    "sin" => Func("cos".into(), a.clone()),
                    "cos" => Neg(b(Func("sin".into(), a.clone()))),
                    "tan" => Div(b(Num(1.0)), b(Pow(b(Func("cos".into(), a.clone())), b(Num(2.0))))),
                    "exp" => Func("exp".into(), a.clone()),
                    "ln" => Div(b(Num(1.0)), a.clone()),
                    "log" => Div(b(Num(1.0)), b(Mul(a.clone(), b(Num(std::f64::consts::LN_10))))),
                    "sqrt" => Div(b(Num(1.0)), b(Mul(b(Num(2.0)), b(Func("sqrt".into(), a.clone()))))),
                    "asin" => Div(b(Num(1.0)), b(root_one_minus_sq(a))),
                    "acos" => Neg(b(Div(b(Num(1.0)), b(root_one_minus_sq(a))))),
                    "atan" => Div(b(Num(1.0)), b(Add(b(Num(1.0)), b(Pow(a.clone(), b(Num(2.0))))))),
                    other => return Err(MathError::Unsupported(format!("derivative of `{other}`"))),
                };
                Mul(b(outer), b(inner))
            }
        })
    }

    /// Constant folding and identity removal (`x*1`, `x+0`, `0*x`, `x^1`, ...).
    pub fn simplify(&self) -> Expr {
        let s = |e: &Expr| e.simplify();
        match self {
            Num(_) | Var(_) => self.clone(),
            Neg(a) => match s(a) {
                Num(n) => Num(-n),
                Neg(inner) => *inner,
                other => Neg(other.into()),
            },
            Add(a, b) => match (s(a), s(b)) {
                (Num(x), Num(y)) => Num(x + y),
                (Num(z), e) | (e, Num(z)) if z == 0.0 => e,
                (x, y) => Add(x.into(), y.into()),
            },
            Sub(a, b) => match (s(a), s(b)) {
                (Num(x), Num(y)) => Num(x - y),
                (e, Num(z)) if z == 0.0 => e,
                (Num(z), e) if z == 0.0 => Neg(e.into()).simplify(),
                (x, y) if x == y => Num(0.0),
                (x, y) => Sub(x.into(), y.into()),
            },
            Mul(a, b) => match (s(a), s(b)) {
                (Num(x), Num(y)) => Num(x * y),
                (Num(z), _) | (_, Num(z)) if z == 0.0 => Num(0.0),
                (Num(o), e) | (e, Num(o)) if o == 1.0 => e,
                (Num(o), e) | (e, Num(o)) if o == -1.0 => Neg(e.into()).simplify(),
                // keep numbers in front: x*2 -> 2*x
                (e, Num(n)) => Mul(Num(n).into(), e.into()),
                (x, y) => Mul(x.into(), y.into()),
            },
            Div(a, b) => match (s(a), s(b)) {
                (Num(x), Num(y)) if y != 0.0 => Num(x / y),
                (Num(z), _) if z == 0.0 => Num(0.0),
                (e, Num(o)) if o == 1.0 => e,
                (x, y) if x == y && !matches!(y, Num(_)) => Num(1.0),
                (x, y) => Div(x.into(), y.into()),
            },
            Pow(a, b) => match (s(a), s(b)) {
                (Num(x), Num(y)) if x.powf(y).is_finite() => Num(x.powf(y)),
                (_, Num(z)) if z == 0.0 => Num(1.0),
                (e, Num(o)) if o == 1.0 => e,
                (x, y) => Pow(x.into(), y.into()),
            },
            Func(f, a) => match s(a) {
                Num(n) if Func(f.clone(), Num(n).into()).eval(&HashMap::new()).is_ok() => Num(Func(f.clone(), Num(n).into()).eval(&HashMap::new()).unwrap()),
                e => Func(f.clone(), e.into()),
            },
        }
    }

    fn prec(&self) -> u8 {
        match self {
            Add(..) | Sub(..) => 1,
            Mul(..) | Div(..) => 2,
            Neg(_) => 3,
            Pow(..) => 4,
            Num(n) if *n < 0.0 => 3,
            _ => 5,
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wrap = |e: &Expr, min: u8| if e.prec() < min { format!("({e})") } else { e.to_string() };
        match self {
            Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Var(x) => write!(f, "{x}"),
            Neg(a) => write!(f, "-{}", wrap(a, 4)),
            Add(a, b) => write!(f, "{}+{}", wrap(a, 1), wrap(b, 1)),
            Sub(a, b) => write!(f, "{}-{}", wrap(a, 1), wrap(b, 2)),
            Mul(a, b) => write!(f, "{}*{}", wrap(a, 2), wrap(b, 3)),
            Div(a, b) => write!(f, "{}/{}", wrap(a, 2), wrap(b, 3)),
            Pow(a, b) => write!(f, "{}^{}", wrap(a, 5), wrap(b, 4)),
            Func(name, a) => write!(f, "{name}({a})"),
        }
    }
}

/// `sqrt(1 - a^2)`
fn root_one_minus_sq(a: &Expr) -> Expr {
    Func("sqrt".into(), Box::new(Sub(Box::new(Num(1.0)), Box::new(Pow(Box::new(a.clone()), Box::new(Num(2.0)))))))
}

// ------------------------------------------------------------------ calculus

/// Definite integral of `e` over `[a, b]` with adaptive Simpson quadrature.
pub fn integrate(e: &Expr, var: &str, a: f64, b: f64) -> Result<f64, MathError> {
    if !a.is_finite() || !b.is_finite() {
        return Err(MathError::Unsupported("infinite bounds".into()));
    }
    if a == b {
        return Ok(0.0);
    }
    let (lo, hi, sign) = if a < b { (a, b, 1.0) } else { (b, a, -1.0) };
    let f = |x: f64| e.eval_at(var, x);
    fn rec(f: &dyn Fn(f64) -> Result<f64, MathError>, a: f64, b: f64, fa: f64, fm: f64, fb: f64, whole: f64, eps: f64, depth: u32) -> Result<f64, MathError> {
        let m = (a + b) / 2.0;
        let (lm, rm) = ((a + m) / 2.0, (m + b) / 2.0);
        let (flm, frm) = (f(lm)?, f(rm)?);
        let left = (m - a) / 6.0 * (fa + 4.0 * flm + fm);
        let right = (b - m) / 6.0 * (fm + 4.0 * frm + fb);
        if depth == 0 || (left + right - whole).abs() <= 15.0 * eps {
            return Ok(left + right + (left + right - whole) / 15.0);
        }
        Ok(rec(f, a, m, fa, flm, fm, left, eps / 2.0, depth - 1)? + rec(f, m, b, fm, frm, fb, right, eps / 2.0, depth - 1)?)
    }
    let (fa, fb, fm) = (f(lo)?, f(hi)?, f((lo + hi) / 2.0)?);
    let whole = (hi - lo) / 6.0 * (fa + 4.0 * fm + fb);
    Ok(sign * rec(&f, lo, hi, fa, fm, fb, whole, 1e-10, 20)?)
}

/// Limit of `e` as `var -> at`, from both sides. Errors if the sides disagree or diverge.
pub fn limit(e: &Expr, var: &str, at: f64) -> Result<f64, MathError> {
    // sample closer and closer; Richardson-free but robust for ordinary functions
    let side = |dir: f64| -> Result<f64, MathError> {
        let mut last = None;
        for k in 3..=7 {
            let h = 10f64.powi(-k) * dir;
            match e.eval_at(var, at + h) {
                Ok(v) => last = Some(v),
                Err(err) if last.is_none() => return Err(err),
                Err(_) => break,
            }
        }
        last.ok_or_else(|| MathError::Eval("no samples".into()))
    };
    let (l, r) = (side(-1.0)?, side(1.0)?);
    let scale = l.abs().max(r.abs()).max(1.0);
    if (l - r).abs() > 1e-4 * scale {
        return Err(MathError::Eval(format!("the limit does not exist (left {l}, right {r})")));
    }
    if l.abs() > 1e8 {
        return Err(MathError::Eval("the limit diverges".into()));
    }
    let v = (l + r) / 2.0;
    // snap numerical noise to a clean integer
    Ok(if (v - v.round()).abs() < 1e-5 { v.round() } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        parse(s).unwrap().eval(&HashMap::new()).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("2+3*4"), 14.0);
        assert_eq!(ev("(2+3)*4"), 20.0);
        assert_eq!(ev("2^3^2"), 512.0);
        assert_eq!(ev("-2^2"), -4.0);
        assert_eq!(ev("2^-1"), 0.5);
        assert_eq!(ev("10/4"), 2.5);
        assert_eq!(ev("1e3 + 2.5e-1"), 1000.25);
        assert_eq!(ev("2(3+4)"), 14.0);
        assert!((ev("sin(pi/2) + ln(e)") - 2.0).abs() < 1e-12);
        assert_eq!(ev("sqrt(16) + abs(-3) + log(1000)"), 10.0);
        assert_eq!(ev("1 + 1"), 2.0);
    }

    #[test]
    fn variables_and_implicit_multiplication() {
        let e = parse("3x^2 + 2x + 1").unwrap();
        assert_eq!(e.eval_at("x", 2.0).unwrap(), 17.0);
        assert!(e.eval(&HashMap::new()).is_err());
    }

    #[test]
    fn errors() {
        for bad in ["", "2+", "(1", "1)", "2 $ 3", "sin", "sin 3", "1..2", &"(".repeat(300), &"1+".repeat(600)] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse("1/0").unwrap().eval(&HashMap::new()).is_err());
        assert!(parse("sqrt(-1)").unwrap().eval(&HashMap::new()).is_err());
        assert!(parse("ln(0)").unwrap().eval(&HashMap::new()).is_err());
    }

    #[test]
    fn derivatives_are_symbolically_correct() {
        let d = |s: &str| parse(s).unwrap().derivative("x").unwrap().to_string();
        assert_eq!(d("x^2"), "2*x");
        assert_eq!(d("3x^2 + 2x + 1"), "3*(2*x)+2");
        assert_eq!(d("5"), "0");
        assert_eq!(d("y"), "0");
        assert_eq!(d("sin(x)"), "cos(x)");
        assert_eq!(d("exp(x)"), "exp(x)");
        assert_eq!(d("ln(x)"), "1/x");
        assert_eq!(d("sin(x)*x"), "cos(x)*x+sin(x)");
        // check numerically against finite differences for harder cases
        for src in ["x^3 - 4x", "sin(x^2)", "x^x", "2^x", "x/(1+x^2)", "atan(x)*exp(-x)", "sqrt(x)/x", "tan(x)", "ln(x)*log(x)"] {
            let e = parse(src).unwrap();
            let de = e.derivative("x").unwrap();
            for x in [0.7, 1.3, 2.1] {
                let h = 1e-6;
                let numeric = (e.eval_at("x", x + h).unwrap() - e.eval_at("x", x - h).unwrap()) / (2.0 * h);
                let symbolic = de.eval_at("x", x).unwrap();
                assert!((numeric - symbolic).abs() < 1e-4 * numeric.abs().max(1.0), "{src} at {x}: {numeric} vs {symbolic}");
            }
        }
    }

    #[test]
    fn printing_roundtrips() {
        for src in ["a-(b-c)", "(a+b)*c", "a/(b*c)", "-(a+b)", "(-a)^2", "a^b^c", "(a^b)^c"] {
            let e = parse(src).unwrap();
            let again = parse(&e.to_string()).unwrap();
            let vars: HashMap<String, f64> = [("a", 2.0), ("b", 3.0), ("c", 5.0)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
            assert!((e.eval(&vars).unwrap() - again.eval(&vars).unwrap()).abs() < 1e-9, "{src} -> {e}");
        }
    }

    #[test]
    fn integrals() {
        let e = parse("x^2").unwrap();
        assert!((integrate(&e, "x", 0.0, 3.0).unwrap() - 9.0).abs() < 1e-8);
        assert!((integrate(&e, "x", 3.0, 0.0).unwrap() + 9.0).abs() < 1e-8);
        assert!((integrate(&parse("sin(x)").unwrap(), "x", 0.0, std::f64::consts::PI).unwrap() - 2.0).abs() < 1e-8);
        assert!((integrate(&parse("1/x").unwrap(), "x", 1.0, std::f64::consts::E).unwrap() - 1.0).abs() < 1e-8);
        assert_eq!(integrate(&e, "x", 2.0, 2.0).unwrap(), 0.0);
        assert!(integrate(&parse("1/x").unwrap(), "x", -1.0, 1.0).is_err(), "pole at 0 is reported");
        assert!(integrate(&e, "x", 0.0, f64::INFINITY).is_err());
    }

    #[test]
    fn limits() {
        assert_eq!(limit(&parse("sin(x)/x").unwrap(), "x", 0.0).unwrap(), 1.0);
        assert_eq!(limit(&parse("(x^2-1)/(x-1)").unwrap(), "x", 1.0).unwrap(), 2.0);
        assert!((limit(&parse("(1+x)^(1/x)").unwrap(), "x", 0.0).unwrap() - std::f64::consts::E).abs() < 1e-3);
        assert!(limit(&parse("abs(x)/x").unwrap(), "x", 0.0).is_err(), "sides disagree");
        assert!(limit(&parse("1/x^2").unwrap(), "x", 0.0).is_err(), "diverges");
    }
}
