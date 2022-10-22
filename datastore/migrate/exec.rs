use anyhow::{Result, Context, bail, ensure};
use chisel_snapshot::schema;
use sqlx::any::AnyKind;
use sqlx::Executor;
use crate::layout;
use crate::sql_writer::SqlWriter;
use super::plan;

pub async fn exec_migration_step(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::Step,
) -> Result<()> {
    match step {
        plan::Step::AddTable(step) => add_table(txn, step).await
            .with_context(|| format!("could not add table {:?}", step.new_table.table_name)),
        plan::Step::RemoveTable(step) => remove_table(txn, step).await
            .with_context(|| format!("could not remove table {:?}", step.old_table_name)),
        plan::Step::AddColumn(step) => add_column(txn, step).await
            .with_context(|| format!("could not add column {:?} to table {:?}",
                step.new_col.col_name, step.table_name)),
        plan::Step::RemoveColumn(step) => remove_column(txn, step).await
            .with_context(|| format!("could not remove column {:?} from table {:?}",
                step.old_col_name, step.table_name)),
        plan::Step::UpdateColumn(step) => update_column(txn, step).await
            .with_context(|| format!("could not update column {:?} of table {:?}",
                step.col_name, step.table_name)),
    }
}

async fn add_table(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::AddTable,
) -> Result<()> {
    let new_table = &step.new_table;
    execute_sql(txn, |sql| {
        sql.write("CREATE TABLE ");
        sql.write(&new_table.table_name);
        sql.write(" (");

        {
            let id_col = &new_table.id_col;
            sql.write(&id_col.col_name);
            sql.write(" ");
            sql.write(&id_repr_to_sql(id_col.repr, sql.kind()));
            sql.write(" PRIMARY KEY");
        }

        for field_col in new_table.field_cols.values() {
            sql.write(", ");
            write_col(sql, &field_col);
        }

        sql.write(")");
        Ok(())
    }).await
}

async fn remove_table(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::RemoveTable,
) -> Result<()> {
    execute_sql(txn, |sql| {
        sql.write("DROP TABLE ");
        sql.write(&step.old_table_name);
        Ok(())
    }).await
}

async fn add_column(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::AddColumn,
) -> Result<()> {
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" ADD COLUMN ");
        write_col(sql, &step.new_col);
        sql.write(" DEFAULT (");
        write_value(sql, step.new_col.repr, step.new_col.nullable, &step.value)
            .context("could not encode the default value")?;
        sql.write(")");
        Ok(())
    }).await
}

async fn remove_column(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::RemoveColumn,
) -> Result<()> {
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" DROP COLUMN ");
        sql.write(&step.old_col_name);
        Ok(())
    }).await
}

async fn update_column(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::UpdateColumn,
) -> Result<()> {
    if let Some(new_nullable) = step.new_nullable {
        match txn.kind() {
            AnyKind::Postgres =>
                update_nullable_postgres(txn, step, new_nullable).await?,
            AnyKind::Sqlite => {
                if new_nullable {
                    update_set_nullable_sqlite(txn, step).await?
                } else {
                    bail!("updating nullable column to a non-nullable column is not implemented")
                }
            },
        }
    }
    Ok(())
}

async fn update_nullable_postgres(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::UpdateColumn,
    new_nullable: bool,
) -> Result<()> {
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" ALTER COLUMN ");
        sql.write(&step.col_name);
        sql.write(if new_nullable { "SET" } else { "DROP" });
        sql.write(" NOT NULL");
        Ok(())
    }).await
}

async fn update_set_nullable_sqlite(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::UpdateColumn,
) -> Result<()> {
    let new_col = &step.new_col;
    let tmp_col_name = layout::Name(format!("__tmp_{}", step.col_name.0));

    // 1. create a new nullable column
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" ADD COLUMN ");
        sql.write(&tmp_col_name);
        sql.write(" ");
        sql.write(&field_repr_to_sql(new_col.repr, sql.kind()));
        Ok(())
    }).await?;

    // 2. copy values from the old column to the new column
    execute_sql(txn, |sql| {
        sql.write("UPDATE ");
        sql.write(&step.table_name);
        sql.write(" SET ");
        sql.write(&tmp_col_name);
        sql.write(" = ");
        sql.write(&step.col_name);
        Ok(())
    }).await?;

    // 3. drop the old column
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" DROP COLUMN ");
        sql.write(&step.col_name);
        Ok(())
    }).await?;

    // 4. rename the new column
    execute_sql(txn, |sql| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" RENAME COLUMN ");
        sql.write(&tmp_col_name);
        sql.write(" TO ");
        sql.write(&step.col_name);
        Ok(())
    }).await?;

    Ok(())
}

fn write_col(sql: &mut SqlWriter, field_col: &layout::FieldColumn) {
    sql.write(&field_col.col_name);
    sql.write(" ");
    sql.write(&field_repr_to_sql(field_col.repr, sql.kind()));
    if !field_col.nullable {
        sql.write(" NOT NULL");
    }
}

fn id_repr_to_sql(repr: layout::IdRepr, _kind: AnyKind) -> String {
    match repr {
        layout::IdRepr::UuidAsText | layout::IdRepr::StringAsText => "text",
    }.into()
}

fn field_repr_to_sql(repr: layout::FieldRepr, kind: AnyKind) -> String {
    match repr {
        layout::FieldRepr::StringAsText
            | layout::FieldRepr::UuidAsText
            | layout::FieldRepr::AsJsonText
            => "TEXT",
        layout::FieldRepr::NumberAsDouble
            | layout::FieldRepr::JsDateAsDouble
            => match kind {
                AnyKind::Postgres => "double precision",
                AnyKind::Sqlite => "REAL",
            },
        layout::FieldRepr::BooleanAsInt => match kind {
            AnyKind::Postgres => "smallint",
            AnyKind::Sqlite => "INTEGER",
        },
    }.into()
}

fn write_value(
    sql: &mut SqlWriter,
    repr: layout::FieldRepr,
    nullable: bool,
    value: &schema::Value,
) -> Result<()> {
    match (repr, value) {
        (layout::FieldRepr::AsJsonText, value) => {
            let json: serde_json::Value = match value {
                schema::Value::String(value) => value.clone().into(),
                schema::Value::Number(value) => match value {
                    schema::NumberValue::Finite(value) => value.clone().into(),
                    schema::NumberValue::NegInf => "-inf".into(),
                    schema::NumberValue::PosInf => "+inf".into(),
                },
                schema::Value::Undefined => serde_json::Value::Null,
            };
            let json_str = serde_json::to_string(&json).unwrap();
            sql.write_literal_str(&json_str)
        },
        (layout::FieldRepr::StringAsText, schema::Value::String(value)) =>
            sql.write_literal_str(value),
        (layout::FieldRepr::NumberAsDouble, schema::Value::Number(value)) => {
            let value = value.as_f64().context("number is not a valid f64")?;
            sql.write_literal_f64(value)
        },
        (_, schema::Value::Undefined) => {
            ensure!(nullable, "cannot store undefined (null) in non-nullable column");
            Ok(sql.write("NULL"))
        },
        (_, _) =>
            bail!("invalid combination of default value and column repr"),
    }
}

async fn execute_sql<F>(txn: &mut sqlx::Transaction<'static, sqlx::Any>, f: F) -> Result<()>
    where F: FnOnce(&mut SqlWriter) -> Result<()>
{
    let mut sql_writer = SqlWriter::new(txn.kind());
    f(&mut sql_writer)?;

    async fn execute(
        txn: &mut sqlx::Transaction<'static, sqlx::Any>,
        sql_text: &str,
    ) -> Result<()> {
        txn.execute(sql_text).await
            .with_context(|| {
                if cfg!(debug_assertions) {
                    format!("could not execute SQL statement {:?}", sql_text)
                } else {
                    "could not execute SQL statement".into()
                }
            })?;
        Ok(())
    }

    execute(txn, &sql_writer.build()).await
}
