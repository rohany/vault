extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn;

#[proc_macro_derive(AsAny)]
/// as_any_macro_derive derives an implementation of the vault::util::AsAny
/// trait on a target struct using the derive(AsAny) field.
pub fn as_any_macro_derive(input: TokenStream) -> TokenStream {
    // Construct a representation of Rust code as a syntax tree.
    let ast: syn::DeriveInput = syn::parse(input).unwrap();
    let name = &ast.ident;
    let gen = quote! {
        impl crate::vault::util::AsAny for #name {
            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
    gen.into()
}
