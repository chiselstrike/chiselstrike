use super::migrate_to_0_13;
use super::schema::*;
use super::{execute, fetch_all};
use anyhow::{bail, Result};
use sqlx::Row;

pub struct MigrateContext<'t, 'c> {
    pub query_builder: &'static dyn sea_query::QueryBuilder,
    pub schema_builder: &'static dyn sea_query::SchemaBuilder,
    pub transaction: &'t mut sqlx::Transaction<'c, sqlx::Any>,
}

// Migrates the database schema from given version and returns the new version or `None` if we are
// already at the latest version.
pub async fn migrate_schema_step(
    ctx: &mut MigrateContext<'_, '_>,
    old_version: &str,
) -> Result<Option<&'static str>> {
    // There is a bit of a mess in the schema versions. The recognized versions are:
    // - "empty" is the special version when the database is completely empty ("the beginning of
    // time")
    // - "0" is the schema at the point that we started versioning (i.e. before version 0.7)
    // - "0.7" is the schema at any point between version 0.7 and 0.12 (inclusive)
    // - "0.12" is the schema at version 0.12
    // - "0.13" is the schema at version 0.13
    Ok(match old_version {
        "empty" => {
            migrate_from_empty_to_0(ctx).await?;
            Some("0")
        }
        "0" => {
            migrate_from_0_to_0_7(ctx).await?;
            Some("0.7")
        }
        "0.7" => {
            migrate_from_0_7_to_0_12(ctx).await?;
            Some("0.12")
        }
        "0.12" => {
            migrate_from_0_12_to_0_13(ctx).await?;
            Some("0.13")
        }
        "0.13" => None,
        _ => bail!("Don't know how to migrate from version {:?}", old_version),
    })
}

async fn migrate_from_empty_to_0(ctx: &mut MigrateContext<'_, '_>) -> Result<()> {
    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Types::Table)
            .col(
                sea_query::ColumnDef::new(Types::TypeId)
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .col(
                sea_query::ColumnDef::new(Types::BackingTable)
                    .text()
                    .unique_key(),
            )
            .col(
                sea_query::ColumnDef::new(Types::ApiVersion)
                    .text()
                    .unique_key(),
            ),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(TypeNames::Table)
            .col(sea_query::ColumnDef::new(TypeNames::TypeId).integer())
            .col(
                sea_query::ColumnDef::new(TypeNames::Name)
                    .text()
                    .unique_key(),
            )
            .foreign_key(
                sea_query::ForeignKey::create()
                    .from(TypeNames::Table, TypeNames::TypeId)
                    .to(Types::Table, Types::TypeId)
                    .on_delete(sea_query::ForeignKeyAction::Cascade),
            ),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Fields::Table)
            .col(
                sea_query::ColumnDef::new(Fields::FieldId)
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .col(sea_query::ColumnDef::new(Fields::FieldType).text())
            .col(sea_query::ColumnDef::new(Fields::DefaultValue).text())
            .col(sea_query::ColumnDef::new(Fields::IsOptional).boolean())
            .col(sea_query::ColumnDef::new(TypeNames::TypeId).integer())
            .foreign_key(
                sea_query::ForeignKey::create()
                    .from(Fields::Table, Fields::TypeId)
                    .to(Types::Table, Types::TypeId)
                    .on_delete(sea_query::ForeignKeyAction::Cascade),
            ),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(FieldNames::Table)
            .col(
                sea_query::ColumnDef::new(FieldNames::FieldName)
                    .text()
                    .unique_key(),
            )
            .col(sea_query::ColumnDef::new(FieldNames::FieldId).integer())
            .foreign_key(
                sea_query::ForeignKey::create()
                    .from(FieldNames::Table, FieldNames::FieldId)
                    .to(Fields::Table, Fields::FieldId)
                    .on_delete(sea_query::ForeignKeyAction::Cascade),
            ),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(FieldLabels::Table)
            .col(sea_query::ColumnDef::new(FieldLabels::LabelName).text()) // Denormalized, to keep it simple.
            .col(sea_query::ColumnDef::new(FieldLabels::FieldId).integer())
            .foreign_key(
                sea_query::ForeignKey::create()
                    .from(FieldLabels::Table, FieldLabels::FieldId)
                    .to(Fields::Table, Fields::FieldId)
                    .on_delete(sea_query::ForeignKeyAction::Cascade),
            ),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Endpoints::Table)
            .if_not_exists()
            .col(
                sea_query::ColumnDef::new(Endpoints::Path)
                    .text()
                    .unique_key(),
            )
            .col(sea_query::ColumnDef::new(Endpoints::Code).text()),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Sessions::Table)
            .if_not_exists()
            .col(
                sea_query::ColumnDef::new(Sessions::Token)
                    .uuid()
                    .primary_key(),
            )
            .col(sea_query::ColumnDef::new(Sessions::UserId).text()),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Policies::Table)
            .col(
                sea_query::ColumnDef::new(Policies::Version)
                    .text()
                    .unique_key(),
            )
            .col(sea_query::ColumnDef::new(Policies::PolicyStr).text()),
    )
    .await?;

    Ok(())
}

async fn migrate_from_0_to_0_7(ctx: &mut MigrateContext<'_, '_>) -> Result<()> {
    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(ChiselVersion::Table)
            .col(
                sea_query::ColumnDef::new(ChiselVersion::VersionId)
                    .text()
                    .unique_key(),
            )
            .col(sea_query::ColumnDef::new(ChiselVersion::Version).text()),
    )
    .await?;

    execute_stmt(
        ctx,
        sea_query::Table::alter()
            .table(Fields::Table)
            .add_column(sea_query::ColumnDef::new(Fields::IsUnique).boolean()),
    )
    .await?;
    Ok(())
}

async fn migrate_from_0_7_to_0_12(ctx: &mut MigrateContext<'_, '_>) -> Result<()> {
    // there were many modifications to the database between Chiselstrike versions 0.7 and 0.12;
    // unfortunately, the schema version stored in the database was not updated, we cannot
    // distinguish between versions!

    // 0.7 -> 0.8: no modifications

    // 0.8 -> 0.9
    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(ApiInfo::Table)
            .if_not_exists()
            .col(
                sea_query::ColumnDef::new(ApiInfo::ApiVersion)
                    .text()
                    .unique_key(),
            )
            .col(sea_query::ColumnDef::new(ApiInfo::AppName).text())
            .col(sea_query::ColumnDef::new(ApiInfo::VersionTag).text()),
    )
    .await?;

    // 0.9 -> 0.10
    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Indexes::Table)
            .if_not_exists()
            .col(
                sea_query::ColumnDef::new(Indexes::IndexId)
                    .integer()
                    .auto_increment()
                    .primary_key(),
            )
            .col(sea_query::ColumnDef::new(Indexes::TypeId).integer())
            .col(sea_query::ColumnDef::new(Indexes::Fields).text())
            .foreign_key(
                sea_query::ForeignKey::create()
                    .from(Indexes::Table, Indexes::TypeId)
                    .to(Types::Table, Types::TypeId)
                    .on_delete(sea_query::ForeignKeyAction::Cascade),
            ),
    )
    .await?;

    // ignore errors if the table was already dropped
    let _: Result<_> = execute_stmt(ctx, sea_query::Table::drop().table(Sessions::Table)).await;

    // 0.10 -> 0.11
    // ignore errors if the table was already renamed
    let _: Result<_> = execute_stmt(
        ctx,
        sea_query::Table::rename().table(Endpoints::Table, Sources::Table),
    )
    .await;

    // 0.11 -> 0.12: no modifications

    Ok(())
}

async fn migrate_from_0_12_to_0_13(ctx: &mut MigrateContext<'_, '_>) -> Result<()> {
    execute_stmt(
        ctx,
        sea_query::Table::create()
            .table(Modules::Table)
            .col(sea_query::ColumnDef::new(Modules::Version).text())
            .col(sea_query::ColumnDef::new(Modules::Url).text())
            .col(sea_query::ColumnDef::new(Modules::Code).text())
            .primary_key(
                sea_query::Index::create()
                    .col(Modules::Version)
                    .col(Modules::Url),
            ),
    )
    .await?;

    let source_rows = fetch_all_stmt(
        ctx,
        sea_query::Query::select()
            .column(Sources::Path)
            .column(Sources::Code)
            .from(Sources::Table),
    )
    .await?;

    let source_rows = source_rows
        .into_iter()
        .map(|row| migrate_to_0_13::SourceRow {
            path: row.get(0),
            code: row.get(1),
        })
        .collect();

    let module_rows = migrate_to_0_13::migrate_sources(source_rows);
    if !module_rows.is_empty() {
        let mut insert = sea_query::Query::insert();
        insert
            .into_table(Modules::Table)
            .columns([Modules::Version, Modules::Url, Modules::Code]);
        for module_row in module_rows.into_iter() {
            insert.values([
                module_row.version_id.into(),
                module_row.url.into(),
                module_row.code.into(),
            ])?;
        }
        fetch_all_stmt(ctx, &insert).await?;
    }

    execute_stmt(ctx, sea_query::Table::drop().table(Sources::Table)).await?;

    Ok(())
}

async fn execute_stmt<S>(ctx: &mut MigrateContext<'_, '_>, stmt: &S) -> Result<()>
where
    S: sea_query::SchemaStatementBuilder,
{
    let sql = stmt.build_any(ctx.schema_builder);
    let query = sqlx::query(&sql);
    execute(ctx.transaction, query).await?;
    Ok(())
}

async fn fetch_all_stmt<S>(
    ctx: &mut MigrateContext<'_, '_>,
    stmt: &S,
) -> Result<Vec<sqlx::any::AnyRow>>
where
    S: sea_query::QueryStatementBuilder,
{
    let (sql, params) = stmt.build_any(ctx.query_builder);

    let mut query = sqlx::query(&sql);
    for param in params.iter() {
        // convert `sea_query::Value`-s into `sqlx` values; we should do this more robustly, but
        // this simple manual translation suffices for our purposes for now
        match param {
            sea_query::Value::String(Some(string)) => query = query.bind(&**string),
            _ => panic!("Unimplemented value: {:?}", param),
        }
    }

    fetch_all(&mut *ctx.transaction, query).await
}
