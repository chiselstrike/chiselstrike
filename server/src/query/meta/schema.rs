// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>

//! Metadata schema definitions.

use sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Iden, Table, TableCreateStatement};

#[derive(Iden)]
enum Types {
    Table,
    TypeId,
    BackingTable,
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

pub fn tables() -> Vec<TableCreateStatement> {
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
    vec![types, type_names, fields, type_fields, field_labels]
}
