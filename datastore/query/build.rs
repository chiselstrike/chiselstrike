use crate::conn::DataConn;
use crate::sql_writer::SqlWriter;
use super::{Query, InputParam, OutputExpr};

#[derive(Debug)]
pub struct QueryBuilder {
    pub sql: SqlWriter,
    inputs: Vec<InputParam>,
    output: Option<OutputExpr>,
}

impl QueryBuilder {
    pub fn new(kind: sqlx::any::AnyKind) -> Self {
        Self {
            sql: SqlWriter::new(kind),
            inputs: Vec::new(),
            output: None,
        }
    }

    pub fn add_input(&mut self, input: InputParam) -> usize {
        let param_idx = self.inputs.len();
        self.inputs.push(input);
        param_idx
    }

    pub fn output(&mut self, output: OutputExpr) {
        assert!(self.output.is_none());
        self.output = Some(output);
    }

    pub fn build(self, conn: &DataConn) -> Query {
        Query {
            schema: conn.layout.schema.clone(),
            sql_text: self.sql.build(),
            inputs: self.inputs,
            output: self.output,
        }
    }
}
