//! The Python-subset AST the grammar interpreter executes.
//!
//! The snapshot stores Python's own `ast` nodes generically (kind name, then fields in
//! `_fields` order); [`convert_def`] turns them into these typed nodes once, at load.

use std::sync::Arc;

use super::value::Value;

/// A statement.
#[derive(Debug)]
pub(crate) enum Stmt {
    Expr(Expr),
    Assign(Vec<Expr>, Expr),
    AugAssign(Expr, BinOp, Expr),
    Return(Option<Expr>),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    For(Expr, Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>, Vec<Stmt>),
    Break,
    Continue,
    Pass,
    Raise(Option<Expr>),
    Try(Box<TryStmt>),
    Def(Box<FuncDef>),
    Import(Vec<(Arc<str>, Option<Arc<str>>)>),
    ImportFrom(Arc<str>, Vec<(Arc<str>, Option<Arc<str>>)>),
    Global(Vec<Arc<str>>),
    Delete(Vec<Expr>),
    Assert(Expr),
    /// A statement kind the interpreter does not run; executing it is an error.
    Unsupported(Arc<str>),
}

/// `try` with its handlers, `else` and `finally` blocks.
#[derive(Debug)]
pub(crate) struct TryStmt {
    pub body: Vec<Stmt>,
    pub handlers: Vec<Handler>,
    pub orelse: Vec<Stmt>,
    pub finalbody: Vec<Stmt>,
}

/// One `except` clause.
#[derive(Debug)]
pub(crate) struct Handler {
    pub class: Option<Expr>,
    pub name: Option<Arc<str>>,
    pub body: Vec<Stmt>,
}

/// A function or lambda definition.
#[derive(Debug)]
pub(crate) struct FuncDef {
    pub name: Arc<str>,
    pub params: Params,
    pub body: Vec<Stmt>,
}

/// Parameters, with the default expressions of definitions executed at run time.
#[derive(Debug, Default)]
pub(crate) struct Params {
    pub positional: Vec<Arc<str>>,
    pub vararg: Option<Arc<str>>,
    pub kwonly: Vec<Arc<str>>,
    pub kwarg: Option<Arc<str>>,
    pub defaults: Vec<Expr>,
    pub kw_defaults: Vec<Option<Expr>>,
}

/// An expression.
#[derive(Debug)]
pub(crate) enum Expr {
    Const(Value),
    Name(Arc<str>),
    Attr(Box<Expr>, Arc<str>),
    Subscript(Box<Expr>, Box<Expr>),
    Slice(Option<Box<Expr>>, Option<Box<Expr>>, Option<Box<Expr>>),
    Call(Box<Expr>, Vec<Arg>, Vec<(Option<Arc<str>>, Expr)>),
    Bin(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnaryOp, Box<Expr>),
    Bool(bool, Vec<Expr>),
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
    IfExp(Box<Expr>, Box<Expr>, Box<Expr>),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Dict(Vec<(Option<Expr>, Expr)>),
    Comp(CompKind, Box<Expr>, Option<Box<Expr>>, Vec<Generator>),
    Lambda(Box<FuncDef>),
    Starred(Box<Expr>),
    Joined(Vec<Expr>),
    Formatted(Box<Expr>, i64, Option<Box<Expr>>),
    /// An expression kind the interpreter does not evaluate; evaluating it is an error.
    Unsupported(Arc<str>),
}

/// A call argument.
#[derive(Debug)]
pub(crate) enum Arg {
    Plain(Expr),
    Star(Expr),
}

/// One `for … in … if …` clause of a comprehension.
#[derive(Debug)]
pub(crate) struct Generator {
    pub target: Expr,
    pub iter: Expr,
    pub ifs: Vec<Expr>,
}

/// Comprehension kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompKind {
    List,
    Set,
    Gen,
    Dict,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnaryOp {
    Not,
    Neg,
    Pos,
    Invert,
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    Is,
    IsNot,
    In,
    NotIn,
}
