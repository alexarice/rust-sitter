use serde_json::{json, Map, Value};
use std::path::Path;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    *,
};

#[derive(Debug, Clone, PartialEq)]
struct NameValueExpr {
    pub path: Ident,
    pub eq_token: Token![=],
    pub expr: Expr,
}

impl Parse for NameValueExpr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(NameValueExpr {
            path: input.parse()?,
            eq_token: input.parse()?,
            expr: input.parse()?,
        })
    }
}

fn gen_leaf(path: String, leaf: Field, out: &mut Map<String, Value>) {
    let leaf_attr = leaf
        .attrs
        .iter()
        .find(|attr| attr.path == syn::parse_quote!(rust_sitter::leaf))
        .unwrap();

    let leaf_params = leaf_attr
        .parse_args_with(Punctuated::<NameValueExpr, Token![,]>::parse_terminated)
        .unwrap();

    let pattern_param = leaf_params
        .iter()
        .find(|param| param.path == "pattern")
        .map(|p| &p.expr);

    if let Some(Expr::Lit(lit)) = pattern_param {
        if let Lit::Str(s) = &lit.lit {
            out.insert(
                path,
                json!({
                    "type": "PATTERN",
                    "value": s.value(),
                }),
            );
        } else {
            panic!("Expected pattern to be a string literal");
        }
    } else {
        todo!()
    }
}

fn gen_enum_variant(path: String, variant: Variant, out: &mut Map<String, Value>) {
    variant.fields.iter().enumerate().for_each(|(i, field)| {
        let ident_str = field
            .ident
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or(format!("{}", i));
        gen_leaf(
            format!("{}_{}", path.clone(), ident_str),
            field.clone(),
            out,
        );
    });

    let children = variant
        .fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let ident_str = field
                .ident
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or(format!("{}", i));
            let ident = format!("{}_{}", path.clone(), ident_str);
            json!({
                "type": "SYMBOL",
                "name": ident
            })
        })
        .collect::<Vec<Value>>();

    out.insert(
        path,
        json!({
            "type": "SEQ",
            "members": children,
        }),
    );
}

fn generate_grammar(module: &ItemMod) -> Value {
    let mut rules_map = Map::new();

    let (_, contents) = module.content.as_ref().unwrap();

    let root_type = contents
        .iter()
        .find_map(|item| match item {
            Item::Enum(e) => {
                if e.attrs
                    .iter()
                    .any(|attr| attr.path == syn::parse_quote!(rust_sitter::language))
                {
                    Some(e.ident.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .expect("Each parser must have the root type annotated with `#[rust_sitter::language]`")
        .to_string();

    rules_map.insert(
        "source_file".to_string(),
        json!({
            "type": "ALIAS",
            "named": false,
            "value": &root_type,
            "content": {
                "type": "SYMBOL",
                "name": &root_type
            }
        }),
    );

    contents.iter().for_each(|c| match c {
        Item::Enum(e) => {
            e.variants.iter().for_each(|v| {
                gen_enum_variant(
                    format!("{}_{}", e.ident, v.ident),
                    v.clone(),
                    &mut rules_map,
                )
            });

            let mut members: Vec<Value> = vec![];
            e.variants.iter().for_each(|v| {
                let variant_path = format!("{}_{}", e.ident.clone(), v.ident);
                members.push(json!({
                    "type": "SYMBOL",
                    "name": variant_path
                }))
            });

            rules_map.insert(
                e.ident.to_string(),
                json!({
                    "type": "CHOICE",
                    "members": members
                }),
            );
        }

        _ => panic!(),
    });

    json!({
        "name": "grammar",
        "rules": rules_map
    })
}

fn generate_all_grammars(item: &Item, out: &mut Vec<String>) {
    if let Item::Mod(m) = item {
        m.content
            .iter()
            .for_each(|(_, items)| items.iter().for_each(|i| generate_all_grammars(i, out)));

        if m.attrs
            .iter()
            .any(|a| a.path == parse_quote!(rust_sitter::grammar))
        {
            out.push(generate_grammar(m).to_string())
        }
    }
}

pub fn generate_grammars(root_file: &Path) -> Vec<String> {
    let root_file = syn_inline_mod::parse_and_inline_modules(root_file).items;
    let mut out = vec![];
    root_file
        .iter()
        .for_each(|i| generate_all_grammars(i, &mut out));
    out
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::generate_grammar;

    #[test]
    fn enum_transformed_fields() {
        let m = if let syn::Item::Mod(m) = parse_quote! {
            mod ffi {
                #[rust_sitter::language]
                pub enum Expression {
                    Number(
                        #[rust_sitter::leaf(pattern = r"\d+", transform = |v: &str| v.parse::<i32>().unwrap())]
                        i32
                    ),
                }
            }
        } {
            m
        } else {
            panic!()
        };

        insta::assert_display_snapshot!(generate_grammar(&m));
    }
}