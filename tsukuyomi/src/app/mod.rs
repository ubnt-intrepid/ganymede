//! Components for constructing HTTP applications.

pub mod fallback;
pub mod route;
pub mod scope;

/// A *prelude* for using the primitive `Scope`s.
pub mod directives {
    #[doc(no_inline)]
    pub use super::{
        scope::{mount, route},
        App,
    };

    use {
        super::{
            fallback::{Fallback, FallbackInstance},
            scope::Scope,
        },
        crate::{common::Never, modifier::Modifier},
    };

    /// Creates a `Scope` that registers the specified state to be shared into the scope.
    #[allow(deprecated)]
    pub fn state<T>(state: T) -> impl Scope<Error = Never>
    where
        T: Send + Sync + 'static,
    {
        super::scope::raw(move |cx| {
            cx.set_state(state);
            Ok(())
        })
    }

    /// Creates a `Scope` that registers the specified `Modifier` into the scope.
    #[allow(deprecated)]
    pub fn modifier<M>(modifier: M) -> impl Scope<Error = Never>
    where
        M: Modifier + Send + Sync + 'static,
    {
        super::scope::raw(move |cx| {
            cx.add_modifier(modifier);
            Ok(())
        })
    }

    /// Creates a `Scope` that registers the specified `Fallback` into the scope.
    pub fn fallback<F>(fallback: F) -> impl Scope<Error = Never>
    where
        F: Fallback + Send + Sync + 'static,
    {
        state(FallbackInstance::from(fallback))
    }
}

mod builder;
mod error;
pub(crate) mod imp;
mod router;
mod scoped_map;
#[cfg(test)]
mod tests;
mod uri;

#[doc(hidden)]
#[allow(deprecated)]
pub use self::route::Route;

pub use self::{
    builder::Builder,
    error::{Error, Result},
    scope::Scope,
};
use {
    self::router::Router,
    crate::{common::TryFrom, error::Critical, input::RequestBody, output::ResponseBody},
    futures::{Async, Poll},
    http::{Request, Response},
    std::sync::Arc,
    tower_service::{NewService, Service},
};

use self::scoped_map::{ScopeId, ScopedContainer};
use self::uri::Uri;

#[doc(hidden)]
#[deprecated(since = "0.4.2", note = "use `App::builder` instead")]
pub fn app() -> self::builder::Builder<()> {
    self::builder::Builder::default()
}

#[doc(hidden)]
#[deprecated(since = "0.4.2", note = "use `scope::mount` instead")]
#[allow(deprecated)]
pub fn scope() -> self::scope::Builder<()> {
    self::scope::Builder::<()>::default()
}

#[doc(hidden)]
#[deprecated(since = "0.4.2", note = "use `scope::route` instead")]
#[allow(deprecated)]
pub fn route() -> self::route::Builder<()> {
    self::route::Builder::<()>::default()
}

/// The main type which represents an HTTP application.
#[derive(Debug, Clone)]
pub struct App {
    inner: Arc<AppInner>,
}

#[derive(Debug)]
struct AppInner {
    router: Router,
    data: ScopedContainer,
}

impl AppInner {
    fn get_data<T>(&self, id: ScopeId) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.data.get(id)
    }
}

impl App {
    /// Create a `Builder` to configure the instance of `App`.
    pub fn builder() -> Builder<()> {
        Builder::default()
    }

    /// Create a `Builder` with the specified prefix.
    pub fn with_prefix<T>(prefix: T) -> Result<Builder<()>>
    where
        Uri: TryFrom<T>,
    {
        Ok(Self::builder().prefix(Uri::try_from(prefix)?))
    }
}

impl NewService for App {
    type Request = Request<RequestBody>;
    type Response = Response<ResponseBody>;
    type Error = Critical;
    type Service = AppService;
    type InitError = Critical;
    type Future = futures::future::FutureResult<Self::Service, Self::InitError>;

    fn new_service(&self) -> Self::Future {
        futures::future::ok(AppService {
            inner: self.inner.clone(),
        })
    }
}

/// The instance of `Service` generated by `App`.
#[derive(Debug)]
#[cfg_attr(feature = "cargo-clippy", allow(stutter))]
pub struct AppService {
    inner: Arc<AppInner>,
}

impl Service for AppService {
    type Request = Request<RequestBody>;
    type Response = Response<ResponseBody>;
    type Error = Critical;
    type Future = self::imp::AppFuture;

    #[inline]
    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn call(&mut self, request: Self::Request) -> Self::Future {
        self::imp::AppFuture::new(request, self.inner.clone())
    }
}
