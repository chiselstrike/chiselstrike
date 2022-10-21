use anyhow::{Result, Context, bail};
use chisel_snapshot::schema;
use sqlx::Arguments;
use crate::layout;

pub fn encode_field_to_sql(
    repr: layout::FieldRepr,
    nullable: bool,
    value: &schema::Value,
    out_args: &mut sqlx::any::AnyArguments,
) -> Result<()> {
    match (repr, value) {
        (_, schema::Value::Undefined) if nullable =>
            out_args.add(None::<String>),
        (layout::FieldRepr::AsJsonText, value) => {
            let json = match value {
                schema::Value::String(value) => value.clone().into(),
                schema::Value::Number(value) => value.clone().into(),
                schema::Value::Undefined => serde_json::Value::Null,
            };
            let json_str = serde_json::to_string(&json).unwrap();
            out_args.add(json_str);
        },
        (layout::FieldRepr::StringAsText, schema::Value::String(value)) =>
            out_args.add(value.clone()),
        (layout::FieldRepr::NumberAsDouble, schema::Value::Number(value)) => {
            let value = value.as_f64()
                .context("cannot represent a number value as f64")?;
            out_args.add(value)
        },
        (_, _) =>
            bail!("unsupported combination of field repr {:?} (nullable = {:?}) and value {:?}",
                repr, nullable, value),
    }
    Ok(())
}
