use anyhow::{Result, Context, bail};
use sqlx::any::AnyKind;
use sqlx::{Executor, Statement};
use crate::{encode_value, layout};
use crate::sql_writer::SqlWriter;
use crate::util::reduce_args_lifetime;
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
    execute_sql(txn, |sql, _args| {
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
    execute_sql(txn, |sql, _args| {
        sql.write("DROP TABLE ");
        sql.write(&step.old_table_name);
        Ok(())
    }).await
}

async fn add_column(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::AddColumn,
) -> Result<()> {
    execute_sql(txn, |sql, args| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" ADD COLUMN ");
        write_col(sql, &step.new_col);
        sql.write(" DEFAULT ");
        sql.write_param(0);
        encode_value::encode_field_to_sql(step.new_col.repr, step.new_col.nullable, &step.value, args)?;
        Ok(())
    }).await
}

async fn remove_column(
    txn: &mut sqlx::Transaction<'static, sqlx::Any>,
    step: &plan::RemoveColumn,
) -> Result<()> {
    execute_sql(txn, |sql, _args| {
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
    execute_sql(txn, |sql, _args| {
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
    execute_sql(txn, |sql, _args| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" ADD COLUMN ");
        sql.write(&tmp_col_name);
        sql.write(" ");
        sql.write(&field_repr_to_sql(new_col.repr, sql.kind()));
        Ok(())
    }).await?;

    // 2. copy values from the old column to the new column
    execute_sql(txn, |sql, _args| {
        sql.write("UPDATE ");
        sql.write(&step.table_name);
        sql.write(" SET ");
        sql.write(&tmp_col_name);
        sql.write(" = ");
        sql.write(&step.col_name);
        Ok(())
    }).await?;

    // 3. drop the old column
    execute_sql(txn, |sql, _args| {
        sql.write("ALTER TABLE ");
        sql.write(&step.table_name);
        sql.write(" DROP COLUMN ");
        sql.write(&step.col_name);
        Ok(())
    }).await?;

    // 4. rename the new column
    execute_sql(txn, |sql, _args| {
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
            => "text",
        layout::FieldRepr::NumberAsDouble
            | layout::FieldRepr::JsDateAsDouble
            => match kind {
                AnyKind::Postgres => "double precision",
                AnyKind::Sqlite => "real",
            },
        layout::FieldRepr::BooleanAsInt => match kind {
            AnyKind::Postgres => "smallint",
            AnyKind::Sqlite => "integer",
        },
    }.into()
}

async fn execute_sql<F>(txn: &mut sqlx::Transaction<'static, sqlx::Any>, f: F) -> Result<()>
    where F: FnOnce(&mut SqlWriter, &mut sqlx::any::AnyArguments) -> Result<()>
{
    let mut sql_writer = SqlWriter::new(txn.kind());
    let mut sql_args = sqlx::any::AnyArguments::default();
    f(&mut sql_writer, &mut sql_args)?;

    async fn execute(
        txn: &mut sqlx::Transaction<'static, sqlx::Any>,
        sql_text: &str,
        sql_args: sqlx::any::AnyArguments<'static>,
    ) -> Result<()> {
        let sql_stmt = txn.prepare(sql_text).await
            .context("could not prepare SQL statement")?
            .to_owned();
        let sql_args = unsafe { reduce_args_lifetime(sql_args) };
        let sql_query = sql_stmt.query_with(sql_args);
        txn.execute(sql_query).await
            .context("could not execute SQL statement")?;

        Ok(())
    }

    execute(txn, &sql_writer.build(), sql_args).await
}
