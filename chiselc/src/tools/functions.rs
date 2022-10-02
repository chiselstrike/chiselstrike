// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use swc_ecmascript::ast::{ArrowExpr, BlockStmtOrExpr};

use super::analysis::control_flow::ControlFlow;
use super::analysis::region::Region;
use super::analysis::stmt_map::StmtMap;

pub struct ArrowFunction<'a> {
    pub orig: &'a ArrowExpr,
    pub stmt_map: StmtMap<'a>,
    pub regions: Region,
}

impl<'a> ArrowFunction<'a> {
    pub fn parse(arrow: &'a ArrowExpr) -> Result<Self> {
        // TODO!: factorize in function body
        match &arrow.body {
            BlockStmtOrExpr::BlockStmt(block) => {
                let (cfg, stmt_map) = ControlFlow::build(&block.stmts)?;
                let regions = Region::from_cfg(&cfg, &stmt_map);
                Ok(Self {
                    orig: arrow,
                    stmt_map,
                    regions,
                })
            }
            BlockStmtOrExpr::Expr(_) => todo!(),
        }
    }
}
