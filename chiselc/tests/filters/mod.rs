mod filter_properties;
mod filter_splitting;
mod transform_filter;

macro_rules! assert_ast_eq {
    ($c1:expr, $c2:expr) => {{
        use swc_common::EqIgnoreSpan;

        let parser = chiselc::parse::ParserContext::new();
        let ast1 = parser.parse($c1.clone().into(), true).unwrap();
        let ast2 = parser.parse($c2.clone().into(), true).unwrap();

        assert!(
            ast1.eq_ignore_span(&ast2),
            "expected:\n {}, but found:\n {}",
            $c1,
            $c2
        );
    }};
}

macro_rules! compile {
    ($code:expr, $($entity:literal),*) => {
        compile!($code, $($entity),*; chiselc::rewrite::Target::TypeScript)
    };

    ($code:expr, $($entity:literal),*; $target:expr) => {
        {
            let mut symbols = chiselc::symbols::Symbols::new();
            $(
                symbols.register_entity($entity);
            )*

                let code = $code.to_string();
            let mut out = Vec::new();
            chiselc::parse::compile(
                code.clone(),
                symbols.clone(),
                $target,
                &mut out
            ).unwrap();

            String::from_utf8(out).unwrap()
        }
    };
}

#[macro_export]
macro_rules! assert_no_transform {
    ($code:expr, $($entity:literal),*) => {
        {
            let compiled = compile!($code, $($entity), *);
            assert_ast_eq!($code, compiled
            );
        }
    };
}

pub(crate) use assert_ast_eq;
pub(crate) use assert_no_transform;
pub(crate) use compile;
