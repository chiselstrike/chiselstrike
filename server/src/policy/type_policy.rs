use std::borrow::Cow;

use crate::types::{Type, TypeSystem};
use anyhow::Result;
use chiselc::parse::ParserContext;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TypePolicy {
    pub policies: chiselc::policies::Policies,
    source: String,
}

struct PolicyTypeSystem<'a> {
    version: String,
    ts: &'a TypeSystem,
}

enum PolicyType<'a> {
    Type(&'a TypeSystem, Type),
    ReqContext,
    String,
    Object,
}

impl<'a> chiselc::policies::Type<'a> for PolicyType<'a> {
    fn get_field_ty(&self, field_name: &str) -> Option<Box<dyn chiselc::policies::Type<'a> + 'a>> {
        match self {
            PolicyType::Type(ts, ty) => match ty {
                Type::Entity(e) => {
                    let field = e.get_field(field_name)?;
                    let field_ty = ts.get(&field.type_id).unwrap();

                    Some(Box::new(PolicyType::Type(ts, field_ty)))
                }
                _ => None,
            },
            PolicyType::ReqContext => {
                // TODO: this is not robust at all! need to find a better way.
                match field_name {
                    "userId" => Some(Box::new(PolicyType::String)),
                    "method" => Some(Box::new(PolicyType::String)),
                    "path" => Some(Box::new(PolicyType::String)),
                    "apiVersion" => Some(Box::new(PolicyType::String)),
                    "headers" => Some(Box::new(PolicyType::Object)),
                    _ => None,
                }
            }
            PolicyType::String | PolicyType::Object => None,
        }
    }

    fn name(&self) -> Cow<str> {
        match self {
            PolicyType::Type(_, ty) => Cow::Owned(ty.name()),
            PolicyType::ReqContext => Cow::Borrowed("ReqContext"),
            PolicyType::String => Cow::Borrowed("string"),
            PolicyType::Object => Cow::Borrowed("object"),
        }
    }
}

impl<'a> chiselc::policies::TypeSystem for PolicyTypeSystem<'a> {
    fn get_type<'b>(&'b self, name: &str) -> Box<dyn chiselc::policies::Type<'b> + 'b> {
        if name == "ReqContext" {
            Box::new(PolicyType::ReqContext)
        } else {
            Box::new(PolicyType::Type(
                self.ts,
                self.ts.lookup_type(name, &self.version).unwrap(),
            ))
        }
    }
}

impl TypePolicy {
    pub fn from_policy_code(code: String, ts: &TypeSystem, version: String) -> Result<Self> {
        let ctx = ParserContext::new();
        let module = ctx.parse(code.clone(), false)?;
        let ts = PolicyTypeSystem { version, ts };
        let policies = chiselc::policies::Policies::parse(&module, &ts);
        Ok(Self {
            policies,
            source: code,
        })
    }
}
