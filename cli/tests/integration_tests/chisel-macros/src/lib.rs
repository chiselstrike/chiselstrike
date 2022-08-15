use std::path::PathBuf;

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Ident, ItemFn, LitStr, Token,
};

/// Arguments to the test macro
struct TestArgs {
    /// Variant of the `ModulesSpec` type.
    modules: Ident,
    /// Variant of the `OptimizeSpec` type.
    optimize: Option<Ident>,
}

impl Parse for TestArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut modules = None;
        let mut optimize = None;
        // TODO: use `syn::AttributeArgs` or the `darling` crate to parse the arguments
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "modules" if modules.is_none() => {
                    let _: Token!(=) = input.parse()?;
                    modules = Some(input.parse()?);
                }
                "optimize" if optimize.is_none() => {
                    let _: Token!(=) = input.parse()?;
                    optimize = Some(input.parse()?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unexpected argument: {other}"),
                    ))
                }
            }

            if input.peek(Token!(,)) {
                input.parse::<Token!(,)>()?;
            }
        }

        Ok(Self {
            modules: modules
                .ok_or_else(|| syn::Error::new(input.span(), "missing modules argument"))?,
            optimize,
        })
    }
}

#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as TestArgs);
    let fun = parse_macro_input!(input as ItemFn);
    let fun_name = &fun.sig.ident;
    let fun_name_str = fun_name.to_string();
    let modules = args.modules;
    let optimize = args
        .optimize
        .unwrap_or_else(|| Ident::new("Yes", Span::call_site()));

    quote::quote! {
        ::inventory::submit! {
            crate::suite::TestSpec {
                name: concat!(module_path!(), "::", #fun_name_str),
                modules: crate::suite::ModulesSpec::#modules,
                optimize: crate::suite::OptimizeSpec::#optimize,
                test_fn: &#fun_name,
            }
        }

        #fun
    }
    .into()
}

#[proc_macro]
pub fn mod_dir(input: TokenStream) -> TokenStream {
    let mod_path = parse_macro_input!(input as LitStr);
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push(mod_path.value());
    let mut mods = Vec::new();
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry.unwrap();
        let path = entry.path();
        let ext = path.extension();
        let stem = path.file_stem();
        if let Some((stem, ext)) = stem.zip(ext) {
            if stem != "mod" && ext == "rs" {
                let mod_name = stem.to_str().unwrap();
                mods.push(Ident::new(mod_name, Span::call_site()));
            }
        }
    }
    quote::quote! {
        #(mod #mods ;)*
    }
    .into()
}
