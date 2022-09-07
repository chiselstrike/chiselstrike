// SPDX-FileCopyrightText: © 2021 ChiselStrike <info@chiselstrike.com>
//! Metadata schema definitions.

use sea_query::Iden;

#[derive(Iden)]
pub enum ChiselVersion {
    Table,
    VersionId,
    Version,
}

#[derive(Iden)]
pub enum ApiInfo {
    Table,
    ApiVersion,
    AppName,
    VersionTag,
}

#[derive(Iden)]
pub enum Types {
    Table,
    TypeId,
    BackingTable,
    ApiVersion,
}

#[derive(Iden)]
pub enum TypeNames {
    Table,
    TypeId,
    Name,
}

#[derive(Iden)]
pub enum Fields {
    Table,
    FieldId,
    FieldType,
    TypeId,
    DefaultValue,
    IsOptional,
    IsUnique,
}

#[derive(Iden)]
pub enum FieldNames {
    Table,
    FieldName,
    FieldId,
}

#[derive(Iden)]
pub enum FieldLabels {
    Table,
    LabelName,
    FieldId,
}

#[derive(Iden)]
pub enum Indexes {
    Table,
    IndexId,
    TypeId,
    Fields,
}

#[derive(Iden)]
pub enum Endpoints {
    Table,
    Path,
    Code,
}

#[derive(Iden)]
pub enum Sources {
    Table,
    //Path,
    //Code,
}

#[derive(Iden)]
pub enum Modules {
    Table,
    Version,
    Url,
    Code,
}

#[derive(Iden)]
pub enum Policies {
    Table,
    Version,
    PolicyStr,
}

#[derive(Iden)]
pub enum Sessions {
    Table,
    Token,
    UserId,
}
