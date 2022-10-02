// SPDX-FileCopyrightText: © 2022 ChiselStrike <info@chiselstrike.com>

use anyhow::Result;
use swc_ecmascript::ast::{ArrowExpr, BlockStmtOrExpr, Ident, Pat, TsEntityName, TsType};

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

    /// Returns an iterator over the param name and type of the function.
    pub fn params(&self) -> impl Iterator<Item = (&Ident, Option<&Ident>)> {
        self.orig.params.iter().map(|p| match p {
            Pat::Ident(ident) => {
                let name = &ident.id;
                let ty = ident.type_ann.as_ref().map(|ty| match &*ty.type_ann {
                    TsType::TsTypeRef(ty) => match &ty.type_name {
                        TsEntityName::Ident(ref id) => id,
                        TsEntityName::TsQualifiedName(_) => {
                            unimplemented!("unsupported type name")
                        }
                    },
                    _ => unimplemented!("unsupported type annotation"),
                });

                (name, ty)
            }
            _ => panic!("unsupported function argument"),
        })
    }
}
