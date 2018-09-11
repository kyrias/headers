extern crate proc_macro;
extern crate proc_macro2;
#[macro_use]
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use proc_macro2::Span;
use syn::{Data, Fields, Ident, Meta, NestedMeta};

#[proc_macro_derive(Header, attributes(header))]
pub fn derive_header(input: TokenStream) -> TokenStream {
    let ast = syn::parse(input).unwrap();
    impl_header(&ast).into()
}

fn impl_header(ast: &syn::DeriveInput) -> proc_macro2::TokenStream {
    let fns = match impl_fns(ast) {
        Ok(fns) => fns,
        Err(msg) => {
            return quote! {
                compile_error!(#msg);
            }.into();
        }
    };

    let decode = fns.decode;
    let encode = fns.encode;

    let ty = &ast.ident;
    let hname = to_header_name(&ty.to_string());
    let hname_ident = Ident::new(&hname, Span::call_site());
    let dummy_const = Ident::new(&format!("_IMPL_HEADER_FOR_{}", hname), Span::call_site());
    let impl_block = quote! {
        impl __hc::Header for #ty {
            const NAME: &'static __hc::HeaderName = &__hc::header::#hname_ident;
            fn decode(values: &mut __hc::Values) -> __hc::Result<Self> {
                #decode
            }

            fn encode(&self, values: &mut __hc::ToValues) {
                #encode
            }
        }
    };

    quote! {
        const #dummy_const: () = {
            extern crate headers_core as __hc;
            #impl_block
        };
    }
}

struct Fns {
    encode: proc_macro2::TokenStream,
    decode: proc_macro2::TokenStream,
}

fn impl_fns(ast: &syn::DeriveInput) -> Result<Fns, String> {
    let ty = &ast.ident;

    // Only structs are allowed...
    let st = match ast.data {
        Data::Struct(ref st) => st,
        _ => {
            return Err("derive(Header) only works on structs".into())
        }
    };

    // Check attributes for `#[header(...)]` that may influence the code
    // that is generated...
    let mut is_csv = false;
    for attr in &ast.attrs {
        if attr.path.segments.len() != 1 {
            continue;
        }
        if attr.path.segments[0].ident != "header" {
            continue;
        }

        match attr.interpret_meta() {
            Some(Meta::List(list)) => {
                for meta in &list.nested {
                    match meta {
                        NestedMeta::Meta(Meta::Word(ref word)) if word == "csv" => {
                            is_csv = true;
                        },
                        _ => {
                            return Err("illegal option in #[header(..)] attribute".into())
                        }

                    }
                }

            },
            Some(Meta::NameValue(_)) => {
                return Err("illegal #[header = ..] attribute".into())
            },
            Some(Meta::Word(_)) => {
                return Err("empty #[header] attributes do nothing".into())
            },
            None => {
                // TODO stringify attribute to return better error
                return Err("illegal #[header ??] attribute".into())
            }
        }
    }

    let decode_res = if is_csv {
        quote! {
            __hc::decode::from_comma_delimited(values)
        }
    } else {
        quote! {
            __hc::decode::TryFromValues::try_from_values(values)
        }
    };

    let (decode, encode_name) = match st.fields {
        Fields::Named(ref fields) => {
            if fields.named.len() != 1 {
                return Err("derive(Header) doesn't support multiple fields".into());
            }

            let field = fields
                .named
                .iter()
                .next()
                .expect("just checked for len() == 1");
            let field_name = field.ident.as_ref().unwrap();

            let decode = quote! {
                #decode_res
                    .map(|inner| #ty {
                        #field_name: inner,
                    })
            };

            let encode_name = Ident::new(&field_name.to_string(), Span::call_site());
            (decode, Value::Named(encode_name))
        },
        Fields::Unnamed(ref fields) => {
            if fields.unnamed.len() != 1 {
                return Err("derive(Header) doesn't support multiple fields".into());
            }

            let decode = quote! {
                #decode_res
                    .map(#ty)
            };

            (decode, Value::Unnamed)
        },
        Fields::Unit => {
            return Err("derive(Header) doesn't support unit structs".into())
        }
    };

    let encode = if is_csv {
        let field = if let Value::Named(field) = encode_name {
            quote! {
                (&(self.0).#field)
            }
        } else {
            quote! {
                (&(self.0).0)
            }
        };
        quote! {
            struct __HeaderFmt<'hfmt>(&'hfmt #ty);
            impl<'hfmt> ::std::fmt::Display for __HeaderFmt<'hfmt> {
                fn fmt(&self, hfmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                    __hc::encode::comma_delimited(hfmt, (#field).into_iter())
                }
            }
            values.append_fmt(&__HeaderFmt(self));
        }
    } else {
        let field = if let Value::Named(field) = encode_name {
            quote! {
                (&self.#field)
            }
        } else {
            quote! {
                (&self.0)
            }
        };
        quote! {
            values.append((#field).into());
        }
    };

    Ok(Fns {
        decode,
        encode,
    })
}

fn to_header_name(ty_name: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for c in ty_name.chars() {
        if first {
            out.push(c.to_ascii_uppercase());
            first = false;
        } else {
            if c.is_uppercase() {
                out.push('_');
            }
            out.push(c.to_ascii_uppercase());
        }
    }
    out
}

enum Value {
    Named(Ident),
    Unnamed,
}

