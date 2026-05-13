//! Proc-macro `#[derive(FromRow)]` para medoo_rs.
//!
//! Soporta tipos: `i8..i64`, `u8..u32`, `f32`, `f64`, `bool`, `String`,
//! `Vec<u8>`, y `Option<T>` para cualquiera de los anteriores.
//!
//! Atributo opcional: `#[medoo(rename = "col_name")]` para renombrar
//! la columna leída de la fila. Por defecto usa el nombre del campo.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TS2;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericArgument, Lit, Meta, PathArguments, Type};

#[proc_macro_derive(FromRow, attributes(medoo))]
pub fn derive_from_row(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(n) => &n.named,
            _ => return err(input.span(), "FromRow solo soporta structs con campos nombrados"),
        },
        _ => return err(input.span(), "FromRow solo soporta structs"),
    };

    let mut field_inits = Vec::new();
    for f in fields {
        let ident = match &f.ident {
            Some(i) => i,
            None => continue,
        };
        // Resolver nombre de columna: #[medoo(rename = "x")] o ident.
        let mut col_name = ident.to_string();
        for attr in &f.attrs {
            if !attr.path().is_ident("medoo") {
                continue;
            }
            let nested = attr.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            );
            if let Ok(list) = nested {
                for m in list {
                    if let Meta::NameValue(nv) = m {
                        if nv.path.is_ident("rename") {
                            if let syn::Expr::Lit(syn::ExprLit { lit: Lit::Str(s), .. }) = &nv.value {
                                col_name = s.value();
                            }
                        }
                    }
                }
            }
        }

        let init = build_field_init(ident, &col_name, &f.ty);
        field_inits.push(init);
    }

    let expanded = quote! {
        impl ::medoo_rs::runtime::FromRow for #name {
            fn from_row(__row: &::medoo_rs::runtime::Row) -> ::medoo_rs::Result<Self> {
                use ::medoo_rs::runtime::RowExt as _;
                Ok(Self {
                    #(#field_inits),*
                })
            }
        }
    };
    TokenStream::from(expanded)
}

fn build_field_init(ident: &syn::Ident, col: &str, ty: &Type) -> TS2 {
    let kind = classify(ty);
    let col_lit = col;
    match kind {
        FieldKind::OptString => quote! {
            #ident: __row.get_str(#col_lit).map(|s| s.to_string())
        },
        FieldKind::OptInt => quote! {
            #ident: __row.get_i64(#col_lit)
        },
        FieldKind::OptIntSized(t) => {
            let cast = syn::Ident::new(&t, proc_macro2::Span::call_site());
            quote! {
                #ident: __row.get_i64(#col_lit).map(|v| v as #cast)
            }
        }
        FieldKind::OptF64 => quote! {
            #ident: __row.get_f64(#col_lit)
        },
        FieldKind::OptF32 => quote! {
            #ident: __row.get_f64(#col_lit).map(|v| v as f32)
        },
        FieldKind::OptBool => quote! {
            #ident: __row.get_bool(#col_lit)
        },
        FieldKind::OptBytes => quote! {
            #ident: match __row.get(#col_lit) {
                Some(::medoo_rs::Value::Bytes(b)) => Some(b.clone()),
                _ => None,
            }
        },

        FieldKind::String => quote! {
            #ident: __row.get_str(#col_lit)
                .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o tipo incorrecto", #col_lit)))?
                .to_string()
        },
        FieldKind::Int => quote! {
            #ident: __row.get_i64(#col_lit)
                .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es int", #col_lit)))?
        },
        FieldKind::IntSized(t) => {
            let cast = syn::Ident::new(&t, proc_macro2::Span::call_site());
            quote! {
                #ident: __row.get_i64(#col_lit)
                    .map(|v| v as #cast)
                    .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es int", #col_lit)))?
            }
        }
        FieldKind::F64 => quote! {
            #ident: __row.get_f64(#col_lit)
                .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es float", #col_lit)))?
        },
        FieldKind::F32 => quote! {
            #ident: __row.get_f64(#col_lit)
                .map(|v| v as f32)
                .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es float", #col_lit)))?
        },
        FieldKind::Bool => quote! {
            #ident: __row.get_bool(#col_lit)
                .ok_or_else(|| ::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es bool", #col_lit)))?
        },
        FieldKind::Bytes => quote! {
            #ident: match __row.get(#col_lit) {
                Some(::medoo_rs::Value::Bytes(b)) => b.clone(),
                _ => return Err(::medoo_rs::QueryError::Driver(format!("col '{}' faltante o no es bytes", #col_lit))),
            }
        },
        FieldKind::Unsupported(msg) => {
            let s = format!("medoo FromRow: tipo no soportado en campo '{}': {}", ident, msg);
            quote_spanned! {ty.span()=> compile_error!(#s) }
        }
    }
}

enum FieldKind {
    String,
    Int,
    IntSized(String),
    F64,
    F32,
    Bool,
    Bytes,
    OptString,
    OptInt,
    OptIntSized(String),
    OptF64,
    OptF32,
    OptBool,
    OptBytes,
    Unsupported(String),
}

fn classify(ty: &Type) -> FieldKind {
    if let Some(inner) = option_inner(ty) {
        return match scalar_kind(inner) {
            Some(ScalarKind::String) => FieldKind::OptString,
            Some(ScalarKind::I64) => FieldKind::OptInt,
            Some(ScalarKind::IntSized(t)) => FieldKind::OptIntSized(t),
            Some(ScalarKind::F64) => FieldKind::OptF64,
            Some(ScalarKind::F32) => FieldKind::OptF32,
            Some(ScalarKind::Bool) => FieldKind::OptBool,
            Some(ScalarKind::Bytes) => FieldKind::OptBytes,
            None => FieldKind::Unsupported(format!("{:?}", inner_path_name(inner))),
        };
    }
    match scalar_kind(ty) {
        Some(ScalarKind::String) => FieldKind::String,
        Some(ScalarKind::I64) => FieldKind::Int,
        Some(ScalarKind::IntSized(t)) => FieldKind::IntSized(t),
        Some(ScalarKind::F64) => FieldKind::F64,
        Some(ScalarKind::F32) => FieldKind::F32,
        Some(ScalarKind::Bool) => FieldKind::Bool,
        Some(ScalarKind::Bytes) => FieldKind::Bytes,
        None => FieldKind::Unsupported(format!("{:?}", inner_path_name(ty))),
    }
}

enum ScalarKind {
    String,
    I64,
    IntSized(String),
    F64,
    F32,
    Bool,
    Bytes,
}

fn scalar_kind(ty: &Type) -> Option<ScalarKind> {
    let name = inner_path_name(ty)?;
    match name.as_str() {
        "String" => Some(ScalarKind::String),
        "i64" => Some(ScalarKind::I64),
        "i8" | "i16" | "i32" => Some(ScalarKind::IntSized(name)),
        "u8" | "u16" | "u32" | "u64" => Some(ScalarKind::IntSized(name)),
        "f64" => Some(ScalarKind::F64),
        "f32" => Some(ScalarKind::F32),
        "bool" => Some(ScalarKind::Bool),
        "Vec" => {
            // detectar Vec<u8>
            if let Type::Path(tp) = ty {
                let last = tp.path.segments.last()?;
                if let PathArguments::AngleBracketed(args) = &last.arguments {
                    if let Some(GenericArgument::Type(inner)) = args.args.first() {
                        if inner_path_name(inner).as_deref() == Some("u8") {
                            return Some(ScalarKind::Bytes);
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn inner_path_name(ty: &Type) -> Option<String> {
    if let Type::Path(tp) = ty {
        return tp.path.segments.last().map(|s| s.ident.to_string());
    }
    None
}

fn option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(tp) = ty {
        let seg = tp.path.segments.last()?;
        if seg.ident != "Option" {
            return None;
        }
        if let PathArguments::AngleBracketed(args) = &seg.arguments {
            if let Some(GenericArgument::Type(inner)) = args.args.first() {
                return Some(inner);
            }
        }
    }
    None
}

fn err(span: proc_macro2::Span, msg: &str) -> TokenStream {
    let m = msg.to_string();
    let ts = quote_spanned! {span=> compile_error!(#m); };
    TokenStream::from(ts)
}
