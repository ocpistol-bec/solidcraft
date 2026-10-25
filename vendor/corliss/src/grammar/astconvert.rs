//! Conversion of generic snapshot nodes to the typed AST.

use std::sync::Arc;

use super::ast::*;
use super::astconv::{GField, GNode, Kinds, cmp_op, constant, unsupported};
use crate::error::Result;

impl Kinds {
    /// Converts a FunctionDef or Lambda node.
    pub fn convert_def(&self, n: &GNode) -> Result<FuncDef> {
        let args = self
            .node(n, "args")
            .ok_or_else(|| unsupported("definition without arguments"))?;
        let params = self.params(args)?;
        if self.name(n) == "Lambda" {
            let body = self
                .node(n, "body")
                .ok_or_else(|| unsupported("empty lambda"))?;
            let body = vec![Stmt::Return(Some(self.expr(body)?))];
            return Ok(FuncDef {
                name: Arc::from("<lambda>"),
                params,
                body,
            });
        }
        let name = self.text(n, "name").unwrap_or_else(|| Arc::from("?"));
        Ok(FuncDef {
            name,
            params,
            body: self.block(n, "body")?,
        })
    }

    pub(crate) fn params(&self, a: &GNode) -> Result<Params> {
        let arg_name = |x: &GNode| self.text(x, "arg").unwrap_or_else(|| Arc::from("?"));
        let mut positional: Vec<Arc<str>> = self
            .nodes(a, "posonlyargs")
            .into_iter()
            .map(arg_name)
            .collect();
        positional.extend(self.nodes(a, "args").into_iter().map(arg_name));
        let kw_defaults = match self.get(a, "kw_defaults") {
            GField::List(items) => items
                .iter()
                .map(|f| match f {
                    GField::Node(x) => self.expr(x).map(Some),
                    _ => Ok(None),
                })
                .collect::<Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        Ok(Params {
            positional,
            vararg: self.node(a, "vararg").map(arg_name),
            kwonly: self
                .nodes(a, "kwonlyargs")
                .into_iter()
                .map(arg_name)
                .collect(),
            kwarg: self.node(a, "kwarg").map(arg_name),
            defaults: self.exprs(a, "defaults")?,
            kw_defaults,
        })
    }

    pub(crate) fn block(&self, n: &GNode, field: &str) -> Result<Vec<Stmt>> {
        self.nodes(n, field)
            .into_iter()
            .map(|s| self.stmt(s))
            .collect()
    }

    pub(crate) fn exprs(&self, n: &GNode, field: &str) -> Result<Vec<Expr>> {
        self.nodes(n, field)
            .into_iter()
            .map(|e| self.expr(e))
            .collect()
    }

    pub(crate) fn opt_expr(&self, n: &GNode, field: &str) -> Result<Option<Expr>> {
        self.node(n, field).map(|e| self.expr(e)).transpose()
    }

    pub(crate) fn req_expr(&self, n: &GNode, field: &str) -> Result<Expr> {
        let e = self
            .node(n, field)
            .ok_or_else(|| unsupported(&format!("{} without {field}", self.name(n))))?;
        self.expr(e)
    }

    pub(crate) fn stmt(&self, n: &GNode) -> Result<Stmt> {
        Ok(match self.name(n) {
            "Expr" => Stmt::Expr(self.req_expr(n, "value")?),
            "Assign" => Stmt::Assign(self.exprs(n, "targets")?, self.req_expr(n, "value")?),
            "AugAssign" => Stmt::AugAssign(
                self.req_expr(n, "target")?,
                self.bin_op(n)?,
                self.req_expr(n, "value")?,
            ),
            "Return" => Stmt::Return(self.opt_expr(n, "value")?),
            "If" => Stmt::If(
                self.req_expr(n, "test")?,
                self.block(n, "body")?,
                self.block(n, "orelse")?,
            ),
            "For" => Stmt::For(
                self.req_expr(n, "target")?,
                self.req_expr(n, "iter")?,
                self.block(n, "body")?,
                self.block(n, "orelse")?,
            ),
            "While" => Stmt::While(
                self.req_expr(n, "test")?,
                self.block(n, "body")?,
                self.block(n, "orelse")?,
            ),
            "Break" => Stmt::Break,
            "Continue" => Stmt::Continue,
            "Pass" => Stmt::Pass,
            "Raise" => Stmt::Raise(self.opt_expr(n, "exc")?),
            "Try" | "TryStar" => Stmt::Try(Box::new(self.try_stmt(n)?)),
            "FunctionDef" => Stmt::Def(Box::new(self.convert_def(n)?)),
            "Import" => Stmt::Import(self.aliases(n)),
            "ImportFrom" => Stmt::ImportFrom(
                self.text(n, "module").unwrap_or_else(|| Arc::from("")),
                self.aliases(n),
            ),
            "Global" | "Nonlocal" => Stmt::Global(self.texts(n, "names")),
            "Delete" => Stmt::Delete(self.exprs(n, "targets")?),
            "Assert" => Stmt::Assert(self.req_expr(n, "test")?),
            other => Stmt::Unsupported(Arc::from(other)),
        })
    }

    pub(crate) fn try_stmt(&self, n: &GNode) -> Result<TryStmt> {
        let handlers = self
            .nodes(n, "handlers")
            .into_iter()
            .map(|h| {
                Ok(Handler {
                    class: self.opt_expr(h, "type")?,
                    name: self.text(h, "name"),
                    body: self.block(h, "body")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(TryStmt {
            body: self.block(n, "body")?,
            handlers,
            orelse: self.block(n, "orelse")?,
            finalbody: self.block(n, "finalbody")?,
        })
    }

    pub(crate) fn aliases(&self, n: &GNode) -> Vec<(Arc<str>, Option<Arc<str>>)> {
        self.nodes(n, "names")
            .into_iter()
            .map(|a| {
                (
                    self.text(a, "name").unwrap_or_else(|| Arc::from("?")),
                    self.text(a, "asname"),
                )
            })
            .collect()
    }

    pub(crate) fn op_name(&self, n: &GNode, field: &str) -> String {
        self.node(n, field)
            .map(|o| self.name(o).to_string())
            .unwrap_or_default()
    }

    pub(crate) fn bin_op(&self, n: &GNode) -> Result<BinOp> {
        Ok(match self.op_name(n, "op").as_str() {
            "Add" => BinOp::Add,
            "Sub" => BinOp::Sub,
            "Mult" => BinOp::Mul,
            "Div" => BinOp::Div,
            "FloorDiv" => BinOp::FloorDiv,
            "Mod" => BinOp::Mod,
            "Pow" => BinOp::Pow,
            "LShift" => BinOp::LShift,
            "RShift" => BinOp::RShift,
            "BitOr" => BinOp::BitOr,
            "BitXor" => BinOp::BitXor,
            "BitAnd" => BinOp::BitAnd,
            other => return Err(unsupported(other)),
        })
    }

    pub(crate) fn expr(&self, n: &GNode) -> Result<Expr> {
        Ok(match self.name(n) {
            "Constant" => match constant(self.get(n, "value")) {
                Ok(v) => Expr::Const(v),
                Err(_) => Expr::Unsupported(Arc::from("constant")),
            },
            "Name" => Expr::Name(self.text(n, "id").unwrap_or_else(|| Arc::from("?"))),
            "Attribute" => Expr::Attr(
                Box::new(self.req_expr(n, "value")?),
                self.text(n, "attr").unwrap_or_else(|| Arc::from("?")),
            ),
            "Subscript" => Expr::Subscript(
                Box::new(self.req_expr(n, "value")?),
                Box::new(self.req_expr(n, "slice")?),
            ),
            "Slice" => Expr::Slice(
                self.boxed(n, "lower")?,
                self.boxed(n, "upper")?,
                self.boxed(n, "step")?,
            ),
            "Call" => self.call(n)?,
            "BinOp" => Expr::Bin(
                Box::new(self.req_expr(n, "left")?),
                self.bin_op(n)?,
                Box::new(self.req_expr(n, "right")?),
            ),
            "UnaryOp" => Expr::Unary(self.unary_op(n)?, Box::new(self.req_expr(n, "operand")?)),
            "BoolOp" => Expr::Bool(self.op_name(n, "op") == "And", self.exprs(n, "values")?),
            "Compare" => self.compare(n)?,
            "IfExp" => Expr::IfExp(
                Box::new(self.req_expr(n, "test")?),
                Box::new(self.req_expr(n, "body")?),
                Box::new(self.req_expr(n, "orelse")?),
            ),
            "Tuple" => Expr::Tuple(self.exprs(n, "elts")?),
            "List" => Expr::List(self.exprs(n, "elts")?),
            "Set" => Expr::Set(self.exprs(n, "elts")?),
            "Dict" => self.dict(n)?,
            "ListComp" => self.comp(n, CompKind::List)?,
            "SetComp" => self.comp(n, CompKind::Set)?,
            "GeneratorExp" => self.comp(n, CompKind::Gen)?,
            "DictComp" => self.comp(n, CompKind::Dict)?,
            "Lambda" => Expr::Lambda(Box::new(self.convert_def(n)?)),
            "Starred" => Expr::Starred(Box::new(self.req_expr(n, "value")?)),
            "JoinedStr" => Expr::Joined(self.exprs(n, "values")?),
            "FormattedValue" => Expr::Formatted(
                Box::new(self.req_expr(n, "value")?),
                match self.get(n, "conversion") {
                    GField::Int(v) => *v,
                    _ => -1,
                },
                self.boxed(n, "format_spec")?,
            ),
            other => Expr::Unsupported(Arc::from(other)),
        })
    }

    pub(crate) fn boxed(&self, n: &GNode, field: &str) -> Result<Option<Box<Expr>>> {
        Ok(self.opt_expr(n, field)?.map(Box::new))
    }

    pub(crate) fn call(&self, n: &GNode) -> Result<Expr> {
        let args = self
            .nodes(n, "args")
            .into_iter()
            .map(|a| match self.name(a) {
                "Starred" => Ok(Arg::Star(self.req_expr(a, "value")?)),
                _ => Ok(Arg::Plain(self.expr(a)?)),
            })
            .collect::<Result<Vec<_>>>()?;
        let kwargs = self
            .nodes(n, "keywords")
            .into_iter()
            .map(|k| Ok((self.text(k, "arg"), self.req_expr(k, "value")?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Expr::Call(
            Box::new(self.req_expr(n, "func")?),
            args,
            kwargs,
        ))
    }

    pub(crate) fn unary_op(&self, n: &GNode) -> Result<UnaryOp> {
        Ok(match self.op_name(n, "op").as_str() {
            "Not" => UnaryOp::Not,
            "USub" => UnaryOp::Neg,
            "UAdd" => UnaryOp::Pos,
            "Invert" => UnaryOp::Invert,
            other => return Err(unsupported(other)),
        })
    }

    pub(crate) fn compare(&self, n: &GNode) -> Result<Expr> {
        let ops = match self.get(n, "ops") {
            GField::List(items) => items
                .iter()
                .map(|f| match f {
                    GField::Node(o) => cmp_op(self.name(o)),
                    _ => Err(unsupported("comparison operator")),
                })
                .collect::<Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        let rights = self.exprs(n, "comparators")?;
        Ok(Expr::Compare(
            Box::new(self.req_expr(n, "left")?),
            ops.into_iter().zip(rights).collect(),
        ))
    }

    pub(crate) fn dict(&self, n: &GNode) -> Result<Expr> {
        let keys = match self.get(n, "keys") {
            GField::List(items) => items
                .iter()
                .map(|f| match f {
                    GField::Node(k) => self.expr(k).map(Some),
                    _ => Ok(None),
                })
                .collect::<Result<Vec<_>>>()?,
            _ => Vec::new(),
        };
        Ok(Expr::Dict(
            keys.into_iter().zip(self.exprs(n, "values")?).collect(),
        ))
    }

    pub(crate) fn comp(&self, n: &GNode, kind: CompKind) -> Result<Expr> {
        let gens = self
            .nodes(n, "generators")
            .into_iter()
            .map(|g| {
                Ok(Generator {
                    target: self.req_expr(g, "target")?,
                    iter: self.req_expr(g, "iter")?,
                    ifs: self.exprs(g, "ifs")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if kind == CompKind::Dict {
            let key = self.req_expr(n, "key")?;
            let value = self.req_expr(n, "value")?;
            return Ok(Expr::Comp(kind, Box::new(key), Some(Box::new(value)), gens));
        }
        Ok(Expr::Comp(
            kind,
            Box::new(self.req_expr(n, "elt")?),
            None,
            gens,
        ))
    }
}
