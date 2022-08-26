use std::fmt;

use swc_ecmascript::ast::{ArrowExpr, BlockStmtOrExpr};

use crate::Symbol;

use super::analysis::control_flow::ControlFlow;
use super::analysis::d_ir::DIr;
use super::analysis::region::Region;
use super::analysis::stmt_map::StmtMap;

#[derive(Debug)]
pub enum Func<'a> {
    Arrow(ArrowFunction<'a>),
}

impl<'a> Func<'a> {
    pub fn name(&self) -> Option<&Symbol> {
        match self {
            Func::Arrow(_) => None,
        }
    }
}

pub struct ArrowFunction<'a> {
    pub orig: &'a ArrowExpr,
    cfg: ControlFlow,
    pub stmt_map: StmtMap<'a>,
    pub d_ir: DIr,
}

impl<'a> ArrowFunction<'a> {
    pub fn parse(arrow: &'a ArrowExpr) -> Self {
        // TODO!: factorize in function body
        match &arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => {
                let (cfg, stmt_map) = ControlFlow::build(&block.stmts);
                let regions = Region::from_cfg(&cfg, &stmt_map);
                let d_ir = DIr::from_region(&regions, &stmt_map);
                Self {
                    orig: arrow,
                    cfg,
                    stmt_map,
                    d_ir,
                }
            }
            BlockStmtOrExpr::Expr(_) => todo!(),
        }
    }
}

impl fmt::Debug for ArrowFunction<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cfg:\n")?;

        writeln!(f, "{}", self.cfg.dot())?;

        for sym in self.d_ir.syms() {
            writeln!(f, "eedag for {}", &*sym)?;
            writeln!(f, "{}", self.d_ir.dot(self.d_ir.get_root(sym).unwrap()))?;
        }

        Ok(())
    }
}
