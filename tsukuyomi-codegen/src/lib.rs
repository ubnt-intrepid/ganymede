//! Code generation support for Tsukuyomi.

#![feature(proc_macro, use_extern_macros)]

extern crate proc_macro;
extern crate proc_macro2;
#[macro_use]
extern crate syn;
extern crate quote;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::*;

macro_rules! try_quote {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => {
                use proc_macro2::Span;
                use quote::*;
                let msg = e.to_string();
                return Into::into(quote_spanned!(Span::call_site() => compile_error!(#msg)));
            }
        }
    };
}

/// Modifies the signature of a free-standing function to a suitable form as handler function.
///
/// This macro generates a handler function with inserting some processes before
/// and after the provided function, and then replaces the original function with it.
/// The signature of generated handler functions are as follows:
///
/// ```ignore
/// fn(&mut Input) -> Handle
/// ```
///
/// # Examples
///
/// A handler function which will immediately return a `Responder`:
///
/// ```
/// # #![feature(proc_macro, use_extern_macros)]
/// # extern crate tsukuyomi;
/// # extern crate tsukuyomi_codegen;
/// # use tsukuyomi_codegen::handler;
/// use tsukuyomi::output::Responder;
///
/// #[handler]
/// fn handler() -> impl Responder {
///     "Hello"
/// }
/// ```
///
/// ```
/// # #![feature(proc_macro, use_extern_macros)]
/// # extern crate tsukuyomi;
/// # extern crate tsukuyomi_codegen;
/// # use tsukuyomi_codegen::handler;
/// # use tsukuyomi::Input;
/// # use tsukuyomi::output::Responder;
/// #[handler]
/// fn handler(input: &mut Input) -> String {
///     format!("path = {:?}", input.uri().path())
/// }
/// ```
///
/// A handler function which will return a `Future`:
///
/// ```
/// # #![feature(proc_macro, use_extern_macros)]
/// # extern crate tsukuyomi;
/// # extern crate tsukuyomi_codegen;
/// # extern crate futures;
/// # use tsukuyomi_codegen::handler;
/// # use tsukuyomi::{Input, Error};
/// # use futures::Future;
/// #[handler(async)]
/// fn handler(input: &mut Input) -> impl Future<Item = String, Error = Error> + Send + 'static {
///     input.body_mut().read_all().convert_to()
/// }
/// ```
///
/// Uses `futures-await`:
///
/// ```
/// # #![feature(proc_macro, use_extern_macros, proc_macro_non_items, generators)]
/// # extern crate tsukuyomi;
/// # extern crate tsukuyomi_codegen;
/// # extern crate futures_await as futures;
/// # use tsukuyomi_codegen::handler;
/// # use tsukuyomi::Error;
/// #[handler(await)]
/// fn handler() -> Result<&'static str, Error> {
///     Ok("Hello")
/// }
/// ```
#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item: syn::ItemFn = try_quote!(syn::parse(item));
    // FIXME: detect the keyword `async`
    let mode = try_quote!(detect_mode(&attr, &item));

    let context = Context {
        item: item,
        mode: mode,
    };

    try_quote!(context.validate());
    let inner = try_quote!(context.generate_inner());
    let new_item = try_quote!(context.generate_new_item(inner));

    quote!(#new_item).into()
}

// ====

#[derive(Debug, Copy, Clone, PartialEq)]
enum HandlerMode {
    Ready,
    Async,
    AsyncAwait,
}

impl std::str::FromStr for HandlerMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "" | "ready" => Ok(HandlerMode::Ready),
            "async" => Ok(HandlerMode::Async),
            "await" => Ok(HandlerMode::AsyncAwait),
            s => Err(format!("invalid mode: `{}'", s)),
        }
    }
}

fn detect_mode(attr: &TokenStream, _item: &syn::ItemFn) -> Result<HandlerMode, String> {
    attr.to_string().parse()
}

struct Context {
    mode: HandlerMode,
    item: syn::ItemFn,
}

impl Context {
    fn num_inputs(&self) -> usize {
        self.item.decl.inputs.len()
    }

    fn validate(&self) -> Result<(), Box<std::fmt::Display>> {
        if self.num_inputs() > 1 {
            return Err(Box::new("Too many arguments"));
        }

        if self.mode == HandlerMode::AsyncAwait && self.num_inputs() != 0 {
            return Err(Box::new("The number of arguments in #[async] handler must be zero."));
        }

        Ok(())
    }

    fn generate_inner(&self) -> Result<syn::ItemFn, Box<std::fmt::Display>> {
        let mut inner = self.item.clone();

        inner.ident = syn::Ident::new("inner", Span::call_site());

        if self.mode == HandlerMode::AsyncAwait {
            inner.attrs.push(parse_quote!(#[async]));
        }

        Ok(inner)
    }

    fn generate_new_item(&self, inner: syn::ItemFn) -> Result<syn::ItemFn, Box<std::fmt::Display>> {
        let mut new_item = self.item.clone();

        let input_ident: syn::Ident = if self.num_inputs() == 0 && self.mode != HandlerMode::Ready {
            syn::Ident::new("_input", Span::call_site())
        } else {
            syn::Ident::new("input", Span::call_site())
        };
        new_item.decl.inputs = Some(syn::punctuated::Pair::End(parse_quote!(
            #input_ident: &mut ::tsukuyomi::input::Input
        ))).into_iter()
            .collect();

        match new_item.decl.output {
            syn::ReturnType::Default => unimplemented!(),
            syn::ReturnType::Type(_, ref mut ty) => {
                *ty = Box::new(parse_quote!(::tsukuyomi::handler::Handle));
            }
        }

        new_item.block = {
            let call: syn::Expr = match self.num_inputs() {
                0 => parse_quote!(inner()),
                1 => parse_quote!(inner(input)),
                _ => unreachable!(),
            };

            let body: syn::Expr = match self.mode {
                HandlerMode::Ready => parse_quote!({
                    ::tsukuyomi::handler::Handle::ready(
                        ::tsukuyomi::output::Responder::respond_to(#call, input)
                    )
                }),
                HandlerMode::Async | HandlerMode::AsyncAwait => parse_quote!({
                    ::tsukuyomi::handler::Handle::async_responder(#call)
                }),
            };

            let prelude: Option<syn::Stmt> = if self.mode == HandlerMode::AsyncAwait {
                Some(parse_quote!(use futures::prelude::async;))
            } else {
                None
            };

            Box::new(parse_quote!({
                #prelude
                #inner
                #body
            }))
        };

        Ok(new_item)
    }
}