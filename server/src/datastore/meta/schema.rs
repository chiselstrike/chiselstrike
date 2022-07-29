// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

//! Metadata schema definitions.

use sea_query::{
    ColumnDef, ForeignKey, ForeignKeyAction, Iden, Table, TableAlterStatement, TableCreateStatement,
};

#[derive(Iden)]
enum ChiselVersion {
    Table,
    VersionId,
    Version,
}

#[derive(Iden)]
enum ApiInfo {
    Table,
    ApiVersion,
    AppName,
    VersionTag,
}

#[derive(Iden)]
enum Types {
    Table,
    TypeId,
    BackingTable,
    ApiVersion,
}

#[derive(Iden)]
enum TypeNames {
    Table,
    TypeId,
    Name,
}

#[derive(Iden)]
enum Fields {
    Table,
    FieldId,
    FieldType,
    TypeId,
    DefaultValue,
    IsOptional,
    IsUnique,
    JunctionTable,
}

#[derive(Iden)]
enum FieldNames {
    Table,
    FieldName,
    FieldId,
}

#[derive(Iden)]
enum FieldLabels {
    Table,
    LabelName,
    FieldId,
}

#[derive(Iden)]
enum Indexes {
    Table,
    IndexId,
    TypeId,
    Fields,
}

#[derive(Iden)]
enum Sources {
    Table,
    Path,
    Code,
}

#[derive(Iden)]
enum Policies {
    Table,
    Version,
    PolicyStr,
}

pub(crate) static CURRENT_VERSION: &str = "0.7";

// Evolves from a version and returns the new version it evolved to
//
// The intention is to evolve from one version to the one immediately following, which is the only
// way we can keep tests of this sane over the long run. So don't try to be smart and skip
// versions.
pub(crate) async fn evolve_from(
    version: &str,
) -> anyhow::Result<(Vec<TableAlterStatement>, String)> {
    match version {
        "0" => {
            let v = vec![Table::alter()
                .table(Fields::Table)
                .add_column(ColumnDef::new(Fields::IsUnique).boolean())
                .to_owned()];
            Ok((v, "0.7".to_string()))
        }
        v => anyhow::bail!("Don't know how to evolve from version {}", v),
    }
}

pub(crate) fn tables() -> Vec<TableCreateStatement> {
    let version = Table::create()
        .table(ChiselVersion::Table)
        .if_not_exists()
        .col(ColumnDef::new(ChiselVersion::VersionId).text().unique_key())
        .col(ColumnDef::new(ChiselVersion::Version).text())
        .to_owned();

    let api_info = Table::create()
        .table(ApiInfo::Table)
        .if_not_exists()
        .col(ColumnDef::new(ApiInfo::ApiVersion).text().unique_key())
        .col(ColumnDef::new(ApiInfo::AppName).text())
        .col(ColumnDef::new(ApiInfo::VersionTag).text())
        .to_owned();

    let types = Table::create()
        .table(Types::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(Types::TypeId)
                .integer()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(Types::BackingTable).text().unique_key())
        .col(ColumnDef::new(Types::ApiVersion).text().unique_key())
        .to_owned();
    let type_names = Table::create()
        .table(TypeNames::Table)
        .if_not_exists()
        .col(ColumnDef::new(TypeNames::TypeId).integer())
        .col(ColumnDef::new(TypeNames::Name).text().unique_key())
        .foreign_key(
            ForeignKey::create()
                .from(TypeNames::Table, TypeNames::TypeId)
                .to(Types::Table, Types::TypeId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    let fields = Table::create()
        .table(Fields::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(Fields::FieldId)
                .integer()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(Fields::FieldType).text())
        .col(ColumnDef::new(Fields::DefaultValue).text())
        .col(ColumnDef::new(Fields::IsOptional).boolean())
        .col(ColumnDef::new(Fields::IsUnique).boolean())
        .col(ColumnDef::new(Fields::JunctionTable).text())
        .col(ColumnDef::new(TypeNames::TypeId).integer())
        .foreign_key(
            ForeignKey::create()
                .from(Fields::Table, Fields::TypeId)
                .to(Types::Table, Types::TypeId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    let type_fields = Table::create()
        .table(FieldNames::Table)
        .if_not_exists()
        .col(ColumnDef::new(FieldNames::FieldName).text().unique_key())
        .col(ColumnDef::new(FieldNames::FieldId).integer())
        .foreign_key(
            ForeignKey::create()
                .from(FieldNames::Table, FieldNames::FieldId)
                .to(Fields::Table, Fields::FieldId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    let field_labels = Table::create()
        .table(FieldLabels::Table)
        .if_not_exists()
        .col(ColumnDef::new(FieldLabels::LabelName).text()) // Denormalized, to keep it simple.
        .col(ColumnDef::new(FieldLabels::FieldId).integer())
        .foreign_key(
            ForeignKey::create()
                .from(FieldLabels::Table, FieldLabels::FieldId)
                .to(Fields::Table, Fields::FieldId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    let indexes = Table::create()
        .table(Indexes::Table)
        .if_not_exists()
        .col(
            ColumnDef::new(Indexes::IndexId)
                .integer()
                .auto_increment()
                .primary_key(),
        )
        .col(ColumnDef::new(Indexes::TypeId).integer())
        .col(ColumnDef::new(Indexes::Fields).text())
        .foreign_key(
            ForeignKey::create()
                .from(Indexes::Table, Indexes::TypeId)
                .to(Types::Table, Types::TypeId)
                .on_delete(ForeignKeyAction::Cascade),
        )
        .to_owned();
    let sources = Table::create()
        .table(Sources::Table)
        .if_not_exists()
        .col(ColumnDef::new(Sources::Path).text().unique_key())
        .col(ColumnDef::new(Sources::Code).text())
        .to_owned();

    let policies = Table::create()
        .table(Policies::Table)
        .if_not_exists()
        .col(ColumnDef::new(Policies::Version).text().unique_key())
        .col(ColumnDef::new(Policies::PolicyStr).text())
        .to_owned();

    vec![
        version,
        api_info,
        types,
        type_names,
        fields,
        type_fields,
        field_labels,
        indexes,
        sources,
        policies,
    ]
}
