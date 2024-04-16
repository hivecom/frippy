//! Provides the plugin derive macro
#![recursion_limit = "128"]

extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;

extern crate failure;

use proc_macro::TokenStream;

#[proc_macro_derive(PluginName)]
pub fn derive_plugin(data: TokenStream) -> TokenStream {
    let ast = syn::parse(data).unwrap();
    let gen = expand_plugin(&ast);
    gen.into()
}

fn expand_plugin(ast: &syn::DeriveInput) -> quote::Tokens {
    let name = &ast.ident;
    let generics = &ast.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics PluginName for #name #ty_generics #where_clause {
            fn name(&self) -> &str {
                stringify!(#name)
            }
        }
    }
}

#[proc_macro_derive(Error, attributes(error))]
pub fn derive_error(data: TokenStream) -> TokenStream {
    let ast = syn::parse(data).unwrap();
    let tokens = expand_error(&ast);
    tokens.into()
}

fn expand_error(ast: &syn::DeriveInput) -> quote::Tokens {
    if let syn::Data::Enum(_) = ast.data {
    } else {
        panic!("Error should only be derived on ErrorKind enums");
    };

    let mut name = None;
    for attr in &ast.attrs {
        if let Some(syn::Meta::NameValue(name_value)) = attr.interpret_meta() {
            if name_value.ident == "error" {
                if let syn::Lit::Str(lit) = name_value.lit {
                    name = Some(lit.value());
                }
            }
        }
    }

    let struct_name = if let Some(name) = name {
        syn::Ident::from(name)
    } else {
        panic!("Define the error attribute for all Error derives");
    };

    let enum_name = &ast.ident;

    quote! {
        #[derive(Debug)]
        pub struct #struct_name {
            inner: ::failure::Context<#enum_name>,
        }

        impl #struct_name {
            pub fn kind(&self) -> #enum_name {
                *self.inner.get_context()
            }
        }

        impl From<#enum_name> for #struct_name {
            fn from(kind: #enum_name) -> #struct_name {
                #struct_name {
                    inner: ::failure::Context::new(kind),
                }
            }
        }

        impl From<::failure::Context<#enum_name>> for #struct_name {
            fn from(inner: ::failure::Context<#enum_name>) -> #struct_name {
                #struct_name { inner: inner }
            }
        }

        impl ::failure::Fail for #struct_name {
            fn cause(&self) -> Option<&::failure::Fail> {
                self.inner.cause()
            }

            fn backtrace(&self) -> Option<&::failure::Backtrace> {
                self.inner.backtrace()
            }
        }

        impl ::std::fmt::Display for #struct_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                use std::fmt;
                fmt::Display::fmt(&self.inner, f)
            }
        }
    }
}
