//! Content filters: the `DDSSQL` filter expressions of DDS 1.4, Annex B,
//! evaluated on [`DynamicData`].
//!
//! A [`ContentFilter`] is made from a type, a filter expression and its
//! parameters. The grammar is the `FilterExpression` of Annex B, B.2:
//!
//! ```text
//! Condition  ::= Predicate | Condition 'AND' Condition | Condition 'OR' Condition
//!              | 'NOT' Condition | '(' Condition ')'
//! Predicate  ::= FIELDNAME RelOp Parameter | Parameter RelOp FIELDNAME
//!              | FIELDNAME RelOp FIELDNAME
//!              | FIELDNAME ['NOT'] 'BETWEEN' Parameter 'AND' Parameter
//! RelOp      ::= '=' | '>' | '>=' | '<' | '<=' | '<>' | 'LIKE'
//! Parameter  ::= INTEGERVALUE | CHARVALUE | FLOATVALUE | STRING
//!              | ENUMERATEDVALUE | PARAMETER
//! ```
//!
//! Keywords are not case sensitive. `NOT` binds tighter than `AND`, which
//! binds tighter than `OR`. A FIELDNAME names a member of the struct, with
//! `.` for the members of nested structs and unions (B.2, "Token
//! expression"). Beyond Annex B, this implementation also takes `==` and
//! `!=` (the example of DDSI-RTPS 2.5, 8.7.3.1, uses `==`), `TRUE` and
//! `FALSE` for booleans, and `[n]` after a FIELDNAME for an element of an
//! array or a sequence.
//!
//! The values compare as their types do: numbers as numbers whatever their
//! width, strings and characters by their code points, booleans as 0 and
//! 1, and enums by their values, a quoted literal name (ENUMERATEDVALUE)
//! standing for its value. `LIKE` matches a string to a pattern where `%`
//! stands for any characters and `_` for one. A predicate whose field is
//! not there in a sample (an absent optional member, a member of a union
//! that the discriminator does not select, an element past the end of a
//! sequence) or whose operands cannot be compared is false.
//!
//! A PARAMETER `%n` takes the value of the n+1th expression parameter
//! (B.2), written as a literal would be in the expression (`5`, `2.5`,
//! `'text'`, `'RED'`); a parameter that is not a literal is taken as a
//! string, unquoted.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use super::data::{DynamicData, Value};
use super::ty::{DynamicType, resolve, struct_members};
use crate::types::{EnumType, Primitive, Type};
use crate::{Error, Result};

/// The filter class of DDS SQL filters (DDSI-RTPS 2.5, 8.7.3.1).
pub const DDSSQL: &str = "DDSSQL";

/// The most parameters an expression can have: `%0` to `%99` (B.2).
const MAX_PARAMETERS: usize = 100;

fn bad(what: impl Into<String>) -> Error {
    Error::BadParameter(what.into())
}

// ---------- Tokens -----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// A FIELDNAME, or a keyword.
    Name(String),
    Int(i128),
    Float(f64),
    /// A quoted literal: a STRING, a CHARVALUE or an ENUMERATEDVALUE.
    Quoted(String),
    Param(usize),
    Op(RelOp),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
}

/// A relational operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Like,
}

impl RelOp {
    /// The operator with its operands swapped: `a op b` is `b op' a`.
    fn swapped(self) -> RelOp {
        match self {
            RelOp::Lt => RelOp::Gt,
            RelOp::Le => RelOp::Ge,
            RelOp::Gt => RelOp::Lt,
            RelOp::Ge => RelOp::Le,
            other => other,
        }
    }
}

fn number(text: &str) -> Option<Token> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        let v = i128::from_str_radix(hex, 16).ok()?;
        return Some(Token::Int(if negative { -v } else { v }));
    }
    if !digits.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        let v: i128 = digits.parse().ok()?;
        return Some(Token::Int(if negative { -v } else { v }));
    }
    let v: f64 = digits.parse().ok()?;
    Some(Token::Float(if negative { -v } else { v }))
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            '[' => {
                out.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Token::RBracket);
                i += 1;
            }
            '=' => {
                // `==` too.
                i += if chars.get(i + 1) == Some(&'=') { 2 } else { 1 };
                out.push(Token::Op(RelOp::Eq));
            }
            '!' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token::Op(RelOp::Ne));
                i += 2;
            }
            '<' => match chars.get(i + 1) {
                Some('=') => {
                    out.push(Token::Op(RelOp::Le));
                    i += 2;
                }
                Some('>') => {
                    out.push(Token::Op(RelOp::Ne));
                    i += 2;
                }
                _ => {
                    out.push(Token::Op(RelOp::Lt));
                    i += 1;
                }
            },
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Token::Op(RelOp::Ge));
                    i += 2;
                } else {
                    out.push(Token::Op(RelOp::Gt));
                    i += 1;
                }
            }
            // A STRING starts with a left or right quote and ends with a
            // right quote (B.2).
            '\'' | '`' | '\u{2018}' | '\u{2019}' => {
                let start = i + 1;
                let end = (start..chars.len())
                    .find(|&j| chars[j] == '\'' || chars[j] == '\u{2019}')
                    .ok_or_else(|| bad("a string without its closing quote"))?;
                let s: String = chars[start..end].iter().collect();
                if s.contains('\n') {
                    return Err(bad("a new line in a string"));
                }
                out.push(Token::Quoted(s));
                i = end + 1;
            }
            '%' => {
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
                let n: usize = chars[start..end]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .map_err(|_| bad("a % without a parameter number"))?;
                if n >= MAX_PARAMETERS {
                    return Err(bad("a parameter number of 100 or more"));
                }
                out.push(Token::Param(n));
                i = end;
            }
            '.' if !chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) => {
                out.push(Token::Dot);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' || c == '+' || c == '-' => {
                let start = i;
                i += 1;
                let hex = c == '0' && matches!(chars.get(i), Some('x' | 'X'))
                    || (c == '-' || c == '+')
                        && chars.get(i) == Some(&'0')
                        && matches!(chars.get(i + 1), Some('x' | 'X'));
                while i < chars.len() {
                    let d = chars[i];
                    let exponent_sign =
                        (d == '+' || d == '-') && !hex && matches!(chars[i - 1], 'e' | 'E');
                    if d.is_ascii_alphanumeric() || d == '.' || exponent_sign {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                out.push(number(&text).ok_or_else(|| bad(format!("not a number: {text}")))?);
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Token::Name(chars[start..i].iter().collect()));
            }
            other => return Err(bad(format!("unexpected character {other:?}"))),
        }
    }
    Ok(out)
}

// ---------- Syntax -----------------------------------------------------

/// A FIELDNAME: member names, and element indices.
#[derive(Debug, Clone, PartialEq)]
enum Segment {
    Member(String),
    Index(usize),
}

/// An operand of a predicate.
#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Field(Vec<Segment>),
    Literal(Literal),
    Param(usize),
}

/// A literal value.
#[derive(Debug, Clone, PartialEq)]
enum Literal {
    Int(i128),
    Float(f64),
    Bool(bool),
    /// Quoted: a string, a character or an enum literal.
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Compare(Operand, RelOp, Operand),
    Between {
        field: Operand,
        low: Operand,
        high: Operand,
    },
}

fn keyword(t: Option<&Token>, k: &str) -> bool {
    matches!(t, Some(Token::Name(n)) if n.eq_ignore_ascii_case(k))
}

const KEYWORDS: [&str; 7] = ["AND", "OR", "NOT", "BETWEEN", "LIKE", "TRUE", "FALSE"];

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn peek_at(&self, n: usize) -> Option<&Token> {
        self.tokens.get(self.at + n)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn expect(&mut self, want: Token, what: &str) -> Result<()> {
        if self.next().as_ref() == Some(&want) {
            Ok(())
        } else {
            Err(bad(format!("expected {what}")))
        }
    }

    fn or(&mut self) -> Result<Expr> {
        let mut left = self.and()?;
        while keyword(self.peek(), "OR") {
            self.at += 1;
            left = Expr::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr> {
        let mut left = self.not()?;
        while keyword(self.peek(), "AND") {
            self.at += 1;
            left = Expr::And(Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }

    fn not(&mut self) -> Result<Expr> {
        if keyword(self.peek(), "NOT") {
            self.at += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        if self.peek() == Some(&Token::LParen) {
            self.at += 1;
            let e = self.or()?;
            self.expect(Token::RParen, "')'")?;
            return Ok(e);
        }
        self.predicate()
    }

    fn predicate(&mut self) -> Result<Expr> {
        let left = self.operand()?;
        if let Operand::Field(_) = left {
            let negated = keyword(self.peek(), "NOT") && keyword(self.peek_at(1), "BETWEEN");
            if negated {
                self.at += 1;
            }
            if keyword(self.peek(), "BETWEEN") {
                self.at += 1;
                let low = self.value()?;
                if !keyword(self.peek(), "AND") {
                    return Err(bad("expected AND in BETWEEN"));
                }
                self.at += 1;
                let high = self.value()?;
                let e = Expr::Between {
                    field: left,
                    low,
                    high,
                };
                return Ok(if negated { Expr::Not(Box::new(e)) } else { e });
            }
        }
        let op = match self.next() {
            Some(Token::Op(op)) => op,
            Some(Token::Name(n)) if n.eq_ignore_ascii_case("LIKE") => RelOp::Like,
            _ => return Err(bad("expected a relational operator")),
        };
        let right = self.operand()?;
        match (&left, &right) {
            (Operand::Field(_), _) | (_, Operand::Field(_)) => {}
            _ => return Err(bad("a comparison without a field")),
        }
        Ok(Expr::Compare(left, op, right))
    }

    /// A Parameter: a literal or a `%n`.
    fn value(&mut self) -> Result<Operand> {
        match self.operand()? {
            Operand::Field(_) => Err(bad("expected a value, not a field")),
            v => Ok(v),
        }
    }

    fn operand(&mut self) -> Result<Operand> {
        match self.next() {
            Some(Token::Int(v)) => Ok(Operand::Literal(Literal::Int(v))),
            Some(Token::Float(v)) => Ok(Operand::Literal(Literal::Float(v))),
            Some(Token::Quoted(s)) => Ok(Operand::Literal(Literal::Text(s))),
            Some(Token::Param(n)) => Ok(Operand::Param(n)),
            Some(Token::Name(n)) if n.eq_ignore_ascii_case("TRUE") => {
                Ok(Operand::Literal(Literal::Bool(true)))
            }
            Some(Token::Name(n)) if n.eq_ignore_ascii_case("FALSE") => {
                Ok(Operand::Literal(Literal::Bool(false)))
            }
            Some(Token::Name(n)) if !KEYWORDS.iter().any(|k| n.eq_ignore_ascii_case(k)) => {
                let mut path = vec![Segment::Member(n)];
                loop {
                    match self.peek() {
                        Some(Token::Dot) => {
                            self.at += 1;
                            match self.next() {
                                Some(Token::Name(m)) => path.push(Segment::Member(m)),
                                _ => return Err(bad("expected a member name after '.'")),
                            }
                        }
                        Some(Token::LBracket) => {
                            self.at += 1;
                            match self.next() {
                                Some(Token::Int(i)) if i >= 0 => {
                                    path.push(Segment::Index(i as usize))
                                }
                                _ => return Err(bad("expected an index in '[]'")),
                            }
                            self.expect(Token::RBracket, "']'")?;
                        }
                        _ => break,
                    }
                }
                Ok(Operand::Field(path))
            }
            _ => Err(bad("expected a field name or a value")),
        }
    }
}

/// The literal an expression parameter is: as written in an expression,
/// or else the text itself.
fn parameter_literal(text: &str) -> Literal {
    if let Ok(tokens) = tokenize(text) {
        match tokens.as_slice() {
            [Token::Int(v)] => return Literal::Int(*v),
            [Token::Float(v)] => return Literal::Float(*v),
            [Token::Quoted(s)] => return Literal::Text(s.clone()),
            [Token::Name(n)] if n.eq_ignore_ascii_case("TRUE") => return Literal::Bool(true),
            [Token::Name(n)] if n.eq_ignore_ascii_case("FALSE") => return Literal::Bool(false),
            _ => {}
        }
    }
    Literal::Text(text.to_owned())
}

// ---------- Fields -----------------------------------------------------

/// A step from a value to one inside it, checked against the type.
#[derive(Debug, Clone, PartialEq)]
enum Step {
    /// The member at this index of a struct's members (with its base's).
    Struct(usize),
    /// The member at this index of a union's members, when selected.
    Union(usize),
    /// An element of a sequence or an array.
    Element(usize),
}

/// What a field holds, for comparisons.
#[derive(Debug, Clone, PartialEq)]
enum FieldKind {
    Number,
    Bool,
    Char,
    Text,
    Enum(Arc<EnumType>),
}

fn field_kind(ty: &Type) -> Option<FieldKind> {
    Some(match resolve(ty) {
        Type::Primitive(Primitive::Boolean) => FieldKind::Bool,
        Type::Primitive(Primitive::Char8 | Primitive::Char16) => FieldKind::Char,
        Type::Primitive(_) | Type::Bitmask(_) => FieldKind::Number,
        Type::String { .. } => FieldKind::Text,
        Type::Enum(e) => FieldKind::Enum(e.clone()),
        _ => return None,
    })
}

/// The steps to the field `path` of `ty`, and what it holds.
fn resolve_field(ty: &Type, path: &[Segment]) -> Result<(Vec<Step>, FieldKind)> {
    let name = || {
        path.iter()
            .map(|s| match s {
                Segment::Member(m) => format!(".{m}"),
                Segment::Index(i) => format!("[{i}]"),
            })
            .collect::<String>()
            .trim_start_matches('.')
            .to_owned()
    };
    let mut steps = Vec::new();
    let mut ty = resolve(ty).clone();
    for seg in path {
        let (step, next) = match (seg, &ty) {
            (Segment::Member(m), Type::Struct(s)) => {
                let members = struct_members(s);
                let i = members
                    .iter()
                    .position(|x| x.name == *m)
                    .ok_or_else(|| bad(format!("no field {}", name())))?;
                (Step::Struct(i), members[i].ty.clone())
            }
            (Segment::Member(m), Type::Union(u)) => {
                let i = u
                    .members
                    .iter()
                    .position(|x| x.name == *m)
                    .ok_or_else(|| bad(format!("no field {}", name())))?;
                (Step::Union(i), u.members[i].ty.clone())
            }
            (Segment::Index(i), Type::Sequence(s)) => (Step::Element(*i), s.element.ty.clone()),
            (Segment::Index(i), Type::Array(a)) => {
                if *i >= super::data::array_len(a) {
                    return Err(bad(format!("an index past the array in {}", name())));
                }
                (Step::Element(*i), a.element.ty.clone())
            }
            _ => return Err(bad(format!("no field {}", name()))),
        };
        steps.push(step);
        ty = resolve(&next).clone();
    }
    let kind = field_kind(&ty).ok_or_else(|| {
        bad(format!(
            "{} is not of a primitive, string or enum type",
            name()
        ))
    })?;
    Ok((steps, kind))
}

/// The value of the field at `steps` in `data`, when it is there.
fn field_value<'a>(data: &'a DynamicData, steps: &[Step]) -> Option<&'a DynamicData> {
    let mut d = data;
    for step in steps {
        d = match (step, &d.value) {
            (Step::Struct(i), Value::Struct(values)) => values.get(*i)?.as_ref()?,
            (Step::Union(i), Value::Union { member, .. }) => match member {
                Some((j, v)) if j == i => v,
                _ => return None,
            },
            (Step::Element(i), Value::Collection(items)) => items.get(*i)?,
            _ => return None,
        };
    }
    Some(d)
}

// ---------- Values -----------------------------------------------------

/// A value, ready to compare.
#[derive(Debug, Clone, PartialEq)]
enum Atom {
    Int(i128),
    Float(f64),
    Bool(bool),
    Char(char),
    Text(String),
    Enum(i32, Arc<EnumType>),
}

fn atom_of(d: &DynamicData) -> Option<Atom> {
    Some(match &d.value {
        Value::Boolean(b) => Atom::Bool(*b),
        Value::Byte(x) | Value::Uint8(x) => Atom::Int(i128::from(*x)),
        Value::Int8(x) => Atom::Int(i128::from(*x)),
        Value::Int16(x) => Atom::Int(i128::from(*x)),
        Value::Uint16(x) => Atom::Int(i128::from(*x)),
        Value::Int32(x) => Atom::Int(i128::from(*x)),
        Value::Uint32(x) => Atom::Int(i128::from(*x)),
        Value::Int64(x) => Atom::Int(i128::from(*x)),
        Value::Uint64(x) => Atom::Int(i128::from(*x)),
        Value::Bitmask(x) => Atom::Int(i128::from(*x)),
        Value::Float32(x) => Atom::Float(f64::from(*x)),
        Value::Float64(x) => Atom::Float(*x),
        Value::Float128(x) => Atom::Float(x.to_f64()),
        Value::Char8(c) | Value::Char16(c) => Atom::Char(*c),
        Value::String(s) => Atom::Text(s.clone()),
        Value::Enum(v) => match &d.ty {
            Type::Enum(e) => Atom::Enum(*v, e.clone()),
            _ => Atom::Int(i128::from(*v)),
        },
        _ => return None,
    })
}

fn literal_atom(l: &Literal) -> Atom {
    match l {
        Literal::Int(v) => Atom::Int(*v),
        Literal::Float(v) => Atom::Float(*v),
        Literal::Bool(b) => Atom::Bool(*b),
        Literal::Text(s) => Atom::Text(s.clone()),
    }
}

/// The value of the literal `name` of `e`; a scoped name (`Color::RED`)
/// is taken by its last part.
fn enum_value(e: &EnumType, name: &str) -> Option<i32> {
    let short = name.rsplit("::").next().unwrap_or(name);
    e.literals
        .iter()
        .find(|l| l.name == name || l.name == short)
        .map(|l| l.value)
}

/// A number, for comparing numbers, booleans, characters and enums.
#[derive(Clone, Copy)]
enum Num {
    Int(i128),
    Float(f64),
}

fn as_num(a: &Atom, other: &Atom) -> Option<Num> {
    Some(match a {
        Atom::Int(v) => Num::Int(*v),
        Atom::Float(v) => Num::Float(*v),
        Atom::Bool(b) => Num::Int(i128::from(*b)),
        Atom::Char(c) => Num::Int(i128::from(u32::from(*c))),
        Atom::Enum(v, _) => Num::Int(i128::from(*v)),
        // A quoted literal against a character or an enum: the character
        // itself, or the enum literal of that name.
        Atom::Text(s) => match other {
            Atom::Char(_) => {
                let mut cs = s.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => Num::Int(i128::from(u32::from(c))),
                    _ => return None,
                }
            }
            Atom::Enum(_, e) => Num::Int(i128::from(enum_value(e, s)?)),
            _ => return None,
        },
    })
}

fn compare(a: &Atom, b: &Atom) -> Option<Ordering> {
    if let (Atom::Text(x), Atom::Text(y)) = (a, b) {
        return Some(x.as_str().cmp(y.as_str()));
    }
    match (as_num(a, b)?, as_num(b, a)?) {
        (Num::Int(x), Num::Int(y)) => Some(x.cmp(&y)),
        (Num::Int(x), Num::Float(y)) => (x as f64).partial_cmp(&y),
        (Num::Float(x), Num::Int(y)) => x.partial_cmp(&(y as f64)),
        (Num::Float(x), Num::Float(y)) => x.partial_cmp(&y),
    }
}

/// Whether `text` matches the SQL pattern `pattern`: `%` for any
/// characters, `_` for one.
fn like(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    // The usual backtracking over the last `%`.
    let (mut ti, mut pi) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '_' || (p[pi] != '%' && p[pi] == t[ti])) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '%')
}

fn holds(a: &Atom, op: RelOp, b: &Atom) -> bool {
    if op == RelOp::Like {
        return match (a, b) {
            (Atom::Text(s), Atom::Text(p)) => like(s, p),
            _ => false,
        };
    }
    let Some(o) = compare(a, b) else {
        return false;
    };
    match op {
        RelOp::Eq => o == Ordering::Equal,
        RelOp::Ne => o != Ordering::Equal,
        RelOp::Lt => o == Ordering::Less,
        RelOp::Le => o != Ordering::Greater,
        RelOp::Gt => o == Ordering::Greater,
        RelOp::Ge => o != Ordering::Less,
        RelOp::Like => unreachable!(),
    }
}

// ---------- The filter -------------------------------------------------

/// An operand, its fields resolved.
#[derive(Debug, Clone)]
enum Bound {
    Field(Vec<Step>),
    Literal(Atom),
    Param(usize),
}

#[derive(Debug, Clone)]
enum Node {
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Not(Box<Node>),
    Compare(Bound, RelOp, Bound),
    Between(Bound, Bound, Bound),
}

/// A filter expression (DDS 1.4, Annex B), checked against a type: a
/// `DDSSQL` content filter, with its parameters.
#[derive(Clone)]
pub struct ContentFilter {
    ty: DynamicType,
    expression: String,
    root: Node,
    /// How many parameters the expression uses: its highest `%n`, plus 1.
    parameter_count: usize,
    parameters: Vec<String>,
    values: Vec<Atom>,
}

impl fmt::Debug for ContentFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentFilter")
            .field("type", &self.ty.get_name())
            .field("expression", &self.expression)
            .field("parameters", &self.parameters)
            .finish()
    }
}

fn bind(ty: &Type, o: Operand, count: &mut usize) -> Result<Bound> {
    Ok(match o {
        Operand::Field(path) => Bound::Field(resolve_field(ty, &path)?.0),
        Operand::Literal(l) => Bound::Literal(literal_atom(&l)),
        Operand::Param(n) => {
            *count = (*count).max(n + 1);
            Bound::Param(n)
        }
    })
}

fn build(ty: &Type, e: Expr, count: &mut usize) -> Result<Node> {
    Ok(match e {
        Expr::And(a, b) => Node::And(
            Box::new(build(ty, *a, count)?),
            Box::new(build(ty, *b, count)?),
        ),
        Expr::Or(a, b) => Node::Or(
            Box::new(build(ty, *a, count)?),
            Box::new(build(ty, *b, count)?),
        ),
        Expr::Not(a) => Node::Not(Box::new(build(ty, *a, count)?)),
        Expr::Compare(a, op, b) => {
            // The field first.
            let (a, op, b) = if matches!(a, Operand::Field(_)) {
                (a, op, b)
            } else {
                if op == RelOp::Like {
                    return Err(bad("LIKE with the pattern first"));
                }
                (b, op.swapped(), a)
            };
            Node::Compare(bind(ty, a, count)?, op, bind(ty, b, count)?)
        }
        Expr::Between { field, low, high } => Node::Between(
            bind(ty, field, count)?,
            bind(ty, low, count)?,
            bind(ty, high, count)?,
        ),
    })
}

impl ContentFilter {
    /// Parses `expression` and checks its fields against `ty`, a struct
    /// type, with `parameters` for its `%n`. Fails with
    /// [`Error::BadParameter`] for an expression that is not a filter
    /// expression, a field the type does not have or that is not of a
    /// primitive, string or enum type, or fewer parameters than the
    /// expression uses.
    pub fn new(ty: &DynamicType, expression: &str, parameters: &[String]) -> Result<ContentFilter> {
        if !matches!(resolve(ty.as_type()), Type::Struct(_) | Type::Union(_)) {
            return Err(bad(
                "a content filter on a type that is not a struct or a union",
            ));
        }
        let mut parser = Parser {
            tokens: tokenize(expression)?,
            at: 0,
        };
        let e = parser.or()?;
        if parser.at != parser.tokens.len() {
            return Err(bad("unexpected text after the filter expression"));
        }
        let mut parameter_count = 0;
        let root = build(ty.as_type(), e, &mut parameter_count)?;
        let mut f = ContentFilter {
            ty: ty.clone(),
            expression: expression.to_owned(),
            root,
            parameter_count,
            parameters: Vec::new(),
            values: Vec::new(),
        };
        f.set_parameters(parameters)?;
        Ok(f)
    }

    /// The filter expression.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The expression parameters.
    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }

    /// How many parameters the expression uses: its highest `%n`, plus 1.
    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }

    /// The type the filter is for.
    pub fn get_type(&self) -> &DynamicType {
        &self.ty
    }

    /// Changes the expression parameters. Fails with
    /// [`Error::BadParameter`] when there are fewer than the expression
    /// uses, or more than 100.
    pub fn set_parameters(&mut self, parameters: &[String]) -> Result<()> {
        if parameters.len() < self.parameter_count {
            return Err(bad(format!(
                "the filter expression uses {} parameters, {} given",
                self.parameter_count,
                parameters.len()
            )));
        }
        if parameters.len() > MAX_PARAMETERS {
            return Err(bad("more than 100 parameters"));
        }
        self.values = parameters
            .iter()
            .map(|p| literal_atom(&parameter_literal(p)))
            .collect();
        self.parameters = parameters.to_vec();
        Ok(())
    }

    /// Whether `data` passes the filter. A value of another type than the
    /// filter's does not.
    pub fn evaluate(&self, data: &DynamicData) -> bool {
        data.ty == *resolve(self.ty.as_type()) && self.eval(&self.root, data)
    }

    fn atom(&self, b: &Bound, data: &DynamicData) -> Option<Atom> {
        match b {
            Bound::Field(steps) => atom_of(field_value(data, steps)?),
            Bound::Literal(a) => Some(a.clone()),
            Bound::Param(n) => self.values.get(*n).cloned(),
        }
    }

    fn eval(&self, n: &Node, data: &DynamicData) -> bool {
        match n {
            Node::And(a, b) => self.eval(a, data) && self.eval(b, data),
            Node::Or(a, b) => self.eval(a, data) || self.eval(b, data),
            Node::Not(a) => !self.eval(a, data),
            Node::Compare(a, op, b) => match (self.atom(a, data), self.atom(b, data)) {
                (Some(a), Some(b)) => holds(&a, *op, &b),
                _ => false,
            },
            Node::Between(f, low, high) => {
                match (
                    self.atom(f, data),
                    self.atom(low, data),
                    self.atom(high, data),
                ) {
                    (Some(f), Some(low), Some(high)) => {
                        holds(&f, RelOp::Ge, &low) && holds(&f, RelOp::Le, &high)
                    }
                    _ => false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EnumLiteral, StructMember, StructType};

    fn member(name: &str, id: u32, ty: Type) -> StructMember {
        StructMember::new(name, id, ty)
    }

    fn color() -> Type {
        Type::Enum(Arc::new(EnumType {
            name: "Color".into(),
            literals: ["RED", "GREEN", "BLUE"]
                .iter()
                .enumerate()
                .map(|(i, n)| EnumLiteral {
                    name: (*n).into(),
                    value: i as i32,
                    default: false,
                })
                .collect(),
            ..EnumType::default()
        }))
    }

    fn shape_type() -> DynamicType {
        let point = Type::Struct(Arc::new(StructType {
            name: "Point".into(),
            members: vec![
                member("x", 0, Type::Primitive(Primitive::Int32)),
                member("y", 1, Type::Primitive(Primitive::Float64)),
            ],
            ..StructType::default()
        }));
        DynamicType::new(Type::Struct(Arc::new(StructType {
            name: "Shape".into(),
            members: vec![
                member("name", 0, Type::string()),
                member("size", 1, Type::Primitive(Primitive::Uint32)),
                member("color", 2, color()),
                member("at", 3, point),
                member("flag", 4, Type::Primitive(Primitive::Boolean)),
                member("letter", 5, Type::Primitive(Primitive::Char8)),
            ],
            ..StructType::default()
        })))
    }

    fn shape(name: &str, size: u32, color: i32, x: i32, y: f64) -> DynamicData {
        let ty = shape_type();
        let mut d = super::super::DynamicDataFactory::create_data(&ty);
        d.set_string_value(0, name).unwrap();
        d.set_uint32_value(1, size).unwrap();
        d.set_int32_value(2, color).unwrap();
        let at = d.loan_value(3).unwrap();
        at.set_int32_value(0, x).unwrap();
        at.set_float64_value(1, y).unwrap();
        d.set_boolean_value(4, size > 10).unwrap();
        d.set_char8_value(5, name.chars().next().unwrap_or('?'))
            .unwrap();
        d
    }

    fn passes(expr: &str, params: &[&str], d: &DynamicData) -> bool {
        let params: Vec<String> = params.iter().map(|s| s.to_string()).collect();
        ContentFilter::new(&shape_type(), expr, &params)
            .unwrap()
            .evaluate(d)
    }

    #[test]
    fn comparisons() {
        let d = shape("circle", 20, 2, -3, 1.5);
        assert!(passes("size > 10", &[], &d));
        assert!(!passes("size < 10", &[], &d));
        assert!(passes("size >= 20 AND size <= 20", &[], &d));
        assert!(passes("size = 20", &[], &d));
        assert!(passes("size == 20.0", &[], &d));
        assert!(passes("size <> 21", &[], &d));
        assert!(passes("size != 21", &[], &d));
        assert!(passes("10 < size", &[], &d));
        assert!(passes("name = 'circle'", &[], &d));
        assert!(passes("name > 'apple'", &[], &d));
        assert!(passes("at.x < 0 AND at.y > 1.0e0", &[], &d));
        assert!(passes("at.x = -0x3", &[], &d));
        assert!(passes("color = 'BLUE'", &[], &d));
        assert!(passes("color = 'Color::BLUE'", &[], &d));
        assert!(passes("color > 'RED'", &[], &d));
        assert!(passes("color = 2", &[], &d));
        assert!(!passes("color = 'NOPE'", &[], &d));
        assert!(passes("flag = TRUE", &[], &d));
        assert!(passes("flag = 1", &[], &d));
        assert!(passes("letter = 'c'", &[], &d));
        assert!(passes("letter < 'd'", &[], &d));
        assert!(!passes("name = 5", &[], &d));
        assert!(passes("at.x < at.y", &[], &d));
        assert!(passes("size BETWEEN 10 AND 20", &[], &d));
        assert!(!passes("size NOT BETWEEN 10 AND 20", &[], &d));
        assert!(passes("size not between 21 and 30", &[], &d));
    }

    #[test]
    fn logic_and_like() {
        let d = shape("square", 5, 0, 1, 2.0);
        assert!(passes("name LIKE 'sq%'", &[], &d));
        assert!(passes("name like '%uar_'", &[], &d));
        assert!(passes("name LIKE '%'", &[], &d));
        assert!(!passes("name LIKE 'sq'", &[], &d));
        assert!(passes("name LIKE 's_u%e'", &[], &d));
        assert!(passes("NOT size > 10", &[], &d));
        assert!(passes("size > 10 OR size < 6 AND name = 'square'", &[], &d));
        assert!(!passes("(size > 10 OR size < 6) AND name = 'x'", &[], &d));
        assert!(passes("NOT (size > 10) AND NOT name = 'x'", &[], &d));
    }

    #[test]
    fn parameters() {
        let d = shape("circle", 20, 1, 0, 0.0);
        assert!(passes("size > %0 AND name = %1", &["10", "'circle'"], &d));
        assert!(passes("name = %0", &["circle"], &d));
        assert!(passes("color = %0", &["'GREEN'"], &d));
        assert!(passes("size BETWEEN %0 AND %1", &["1", "2e1"], &d));
        let ty = shape_type();
        let mut f = ContentFilter::new(&ty, "size > %1", &["0".into(), "30".into()]).unwrap();
        assert_eq!(f.parameter_count(), 2);
        assert!(!f.evaluate(&d));
        f.set_parameters(&["0".into(), "3".into()]).unwrap();
        assert!(f.evaluate(&d));
        assert!(f.set_parameters(&["1".into()]).is_err());
        assert!(ContentFilter::new(&ty, "size > %0", &[]).is_err());
    }

    #[test]
    fn errors() {
        let ty = shape_type();
        for e in [
            "",
            "size >",
            "size > 1 AND",
            "nope = 1",
            "at = 1",
            "at.z = 1",
            "size BETWEEN 1",
            "1 = 2",
            "'a' LIKE name",
            "size > 1)",
            "(size > 1",
            "name = 'open",
            "size > %100",
            "size ? 1",
        ] {
            assert!(ContentFilter::new(&ty, e, &[]).is_err(), "{e}");
        }
    }

    #[test]
    fn like_patterns() {
        assert!(like("", ""));
        assert!(like("", "%"));
        assert!(!like("", "_"));
        assert!(like("abc", "a%c"));
        assert!(like("abbbc", "a%b%c"));
        assert!(!like("abc", "a%d"));
        assert!(like("日本語", "日_語"));
    }
}
