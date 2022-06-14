//! AST rewriter that transforms TypeScript code into query expressions.

use crate::filtering::FilterProperties;
use crate::symbols::Symbols;
use crate::transforms::filter::emit::to_ts_expr;
use crate::transforms::filter::infer_filter;
use crate::transforms::find::infer_find;
use std::str::FromStr;
use swc_ecmascript::ast::ExportDefaultDecl;
use swc_ecmascript::ast::FnExpr;
use swc_ecmascript::ast::Function;
use swc_ecmascript::ast::ModuleDecl;
use swc_ecmascript::ast::{
    ArrowExpr, AwaitExpr, BlockStmt, BlockStmtOrExpr, CallExpr, Callee, Decl, DefaultDecl, Expr,
    ExprOrSpread, ExprStmt, MemberExpr, MemberProp, Module, ModuleItem, Stmt, Super, VarDecl,
    VarDeclarator,
};

/// The query language target
#[derive(Clone)]
pub enum Target {
    /// Emit JavaScript using ChiselStrike query expressions.
    JavaScript,
    /// Emit TypeScript using ChiselStrike query expressions.
    TypeScript,
    /// Emit properties that are used in ChiselStrike filter() calls as JSON. The runtime uses this information for auto-indexing purposes.
    FilterProperties,
}

type TargetParseError = &'static str;

impl FromStr for Target {
    type Err = TargetParseError;
    fn from_str(target: &str) -> Result<Self, Self::Err> {
        match target {
            "js" => Ok(Target::JavaScript),
            "ts" => Ok(Target::TypeScript),
            "filter-properties" => Ok(Target::FilterProperties),
            _ => Err("Unknown target"),
        }
    }
}

pub struct Rewriter {
    symbols: Symbols,
    // Accumulated predicate indexes.
    pub indexes: Vec<FilterProperties>,
}

impl Rewriter {
    pub fn new(symbols: Symbols) -> Self {
        Self {
            symbols,
            indexes: vec![],
        }
    }

    pub fn rewrite(&mut self, module: Module) -> Module {
        let mut body = Vec::new();
        for item in module.body {
            body.push(self.rewrite_item(&item));
        }
        Module {
            span: module.span,
            body,
            shebang: module.shebang,
        }
    }

    fn rewrite_item(&mut self, item: &ModuleItem) -> ModuleItem {
        match item {
            ModuleItem::ModuleDecl(decl) => {
                let decl = self.rewrite_module_decl(decl);
                ModuleItem::ModuleDecl(decl)
            }
            ModuleItem::Stmt(stmt) => {
                let stmt = self.rewrite_stmt(stmt);
                ModuleItem::Stmt(stmt)
            }
        }
    }

    fn rewrite_module_decl(&mut self, module_decl: &ModuleDecl) -> ModuleDecl {
        match module_decl {
            ModuleDecl::ExportDefaultDecl(ExportDefaultDecl {
                span,
                decl: DefaultDecl::Fn(fn_expr),
            }) => {
                let fn_expr = self.rewrite_fn_expr(fn_expr);
                ModuleDecl::ExportDefaultDecl(ExportDefaultDecl {
                    span: *span,
                    decl: DefaultDecl::Fn(fn_expr),
                })
            }
            _ => module_decl.clone(),
        }
    }

    fn rewrite_fn_expr(&mut self, fn_expr: &FnExpr) -> FnExpr {
        let body = fn_expr
            .function
            .body
            .as_ref()
            .map(|body| self.rewrite_block_stmt(body));
        FnExpr {
            ident: fn_expr.ident.clone(),
            function: Function {
                params: fn_expr.function.params.clone(),
                decorators: fn_expr.function.decorators.clone(),
                span: fn_expr.function.span,
                body,
                is_generator: fn_expr.function.is_generator,
                is_async: fn_expr.function.is_async,
                type_params: fn_expr.function.type_params.clone(),
                return_type: fn_expr.function.return_type.clone(),
            },
        }
    }

    fn rewrite_stmt(&mut self, stmt: &Stmt) -> Stmt {
        match stmt {
            Stmt::Decl(decl) => {
                let decl = self.rewrite_decl(decl);
                Stmt::Decl(decl)
            }
            Stmt::Expr(expr_stmt) => {
                let expr = self.rewrite_expr(&*expr_stmt.expr);
                let expr_stmt = ExprStmt {
                    span: expr_stmt.span,
                    expr: Box::new(expr),
                };
                Stmt::Expr(expr_stmt)
            }
            _ => stmt.clone(),
        }
    }

    fn rewrite_decl(&mut self, decl: &Decl) -> Decl {
        match decl {
            Decl::Var(var_decl) => {
                let mut decls = Vec::new();
                for decl in &var_decl.decls {
                    let decl = self.rewrite_var_declarator(decl);
                    decls.push(decl);
                }
                Decl::Var(VarDecl {
                    span: var_decl.span,
                    kind: var_decl.kind,
                    declare: var_decl.declare,
                    decls,
                })
            }
            _ => decl.clone(),
        }
    }

    fn rewrite_var_declarator(&mut self, var_declarator: &VarDeclarator) -> VarDeclarator {
        let init = var_declarator
            .init
            .as_ref()
            .map(|init| Box::new(self.rewrite_expr(init)));
        VarDeclarator {
            span: var_declarator.span,
            name: var_declarator.name.clone(),
            init,
            definite: var_declarator.definite,
        }
    }

    fn rewrite_expr(&mut self, expr: &Expr) -> Expr {
        match expr {
            Expr::Arrow(arrow_expr) => {
                let arrow_expr = self.rewrite_arrow_expr(arrow_expr);
                Expr::Arrow(arrow_expr)
            }
            Expr::Await(await_expr) => {
                let await_expr = self.rewrite_await_expr(await_expr);
                Expr::Await(await_expr)
            }
            Expr::Call(call_expr) => {
                let call_expr = self.rewrite_call_expr(call_expr);
                Expr::Call(call_expr)
            }
            Expr::Member(member_expr) => {
                let member_expr = self.rewrite_member_expr(member_expr);
                Expr::Member(member_expr)
            }
            _ => expr.clone(),
        }
    }

    fn rewrite_arrow_expr(&mut self, arrow_expr: &ArrowExpr) -> ArrowExpr {
        let body = match &arrow_expr.body {
            BlockStmtOrExpr::BlockStmt(block_stmt) => {
                let block_stmt = self.rewrite_block_stmt(block_stmt);
                BlockStmtOrExpr::BlockStmt(block_stmt)
            }
            BlockStmtOrExpr::Expr(expr) => {
                let expr = self.rewrite_expr(expr);
                BlockStmtOrExpr::Expr(Box::new(expr))
            }
        };
        ArrowExpr {
            span: arrow_expr.span,
            params: arrow_expr.params.clone(),
            body,
            is_async: arrow_expr.is_async,
            is_generator: arrow_expr.is_generator,
            type_params: arrow_expr.type_params.clone(),
            return_type: arrow_expr.return_type.clone(),
        }
    }

    fn rewrite_block_stmt(&mut self, block_stmt: &BlockStmt) -> BlockStmt {
        let mut stmts = vec![];
        for stmt in &block_stmt.stmts {
            stmts.push(self.rewrite_stmt(stmt));
        }
        BlockStmt {
            span: block_stmt.span,
            stmts,
        }
    }

    fn rewrite_await_expr(&mut self, await_expr: &AwaitExpr) -> AwaitExpr {
        AwaitExpr {
            span: await_expr.span,
            arg: Box::new(self.rewrite_expr(&await_expr.arg)),
        }
    }

    fn rewrite_callee(&mut self, callee: &Callee) -> Callee {
        match callee {
            Callee::Super(Super { span }) => Callee::Super(Super { span: *span }),
            Callee::Import(import) => Callee::Import(*import),
            Callee::Expr(expr) => Callee::Expr(Box::new(self.rewrite_expr(expr))),
        }
    }

    fn rewrite_expr_or_spread(&mut self, expr_or_spread: &ExprOrSpread) -> ExprOrSpread {
        let expr = self.rewrite_expr(&*expr_or_spread.expr);
        ExprOrSpread {
            spread: expr_or_spread.spread,
            expr: Box::new(expr),
        }
    }

    fn rewrite_call_expr(&mut self, call_expr: &CallExpr) -> CallExpr {
        let (filter, index) = infer_filter(call_expr, &self.symbols);
        if let Some(index) = index {
            self.indexes.push(index);
        }
        if let Some(filter) = filter {
            return to_ts_expr(&filter);
        }
        let (filter, index) = infer_find(call_expr, &self.symbols);
        if let Some(index) = index {
            self.indexes.push(index);
        }
        if let Some(filter) = filter {
            return to_ts_expr(&filter);
        }
        let args = call_expr
            .args
            .iter()
            .map(|expr| self.rewrite_expr_or_spread(expr))
            .collect();
        CallExpr {
            span: call_expr.span,
            callee: self.rewrite_callee(&call_expr.callee),
            args,
            type_args: call_expr.type_args.clone(),
        }
    }

    fn rewrite_member_expr(&mut self, member_expr: &MemberExpr) -> MemberExpr {
        MemberExpr {
            span: member_expr.span,
            obj: Box::new(self.rewrite_expr(&member_expr.obj)),
            prop: self.rewrite_member_prop(&member_expr.prop),
        }
    }

    fn rewrite_member_prop(&self, member_prop: &MemberProp) -> MemberProp {
        /* FIXME: Computed property names have expressions */
        member_prop.clone()
    }
}
