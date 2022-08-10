use std::path::PathBuf;

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Ident, ItemFn, LitStr, Path, Token,
};

/// Arguments to the test macro
struct TestArgs {
    /// Mode in which to run the tests: either OpMode::Deno or OpMode::Node.
    mode: Path,
}

impl Parse for TestArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut mode = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            match key.to_string().as_str() {
                "mode" if mode.is_none() => {
                    let _: Token!(=) = input.parse()?;
                    mode.replace(input.parse()?);
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unexpected argument: {other}"),
                    ))
                }
            }
        }

        Ok(Self {
            mode: mode.ok_or_else(|| syn::Error::new(input.span(), "missing mode argument"))?,
        })
    }
}

#[proc_macro_attribute]
pub fn test(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as TestArgs);
    let fun = parse_macro_input!(input as ItemFn);
    let fun_name = &fun.sig.ident;
    let fun_name_str = fun_name.to_string();
    let mode = args.mode;

    quote::quote! {
        inventory::submit! {
            IntegrationTest {
                name: #fun_name_str,
                mode: #mode,
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
