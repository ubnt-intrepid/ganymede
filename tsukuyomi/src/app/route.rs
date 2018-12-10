use {
    super::{
        config::{AppConfig, AppConfigContext},
        uri::{Uri, UriComponent},
        AllowedMethods,
    },
    crate::{
        core::{Chain, Never, TryInto},
        endpoint::Endpoint,
        extractor::Extractor,
        fs::NamedFile,
        future::{Future, MaybeFuture, NeverFuture},
        generic::{Combine, Func},
        handler::{Handler, ModifyHandler},
        input::param::{FromPercentEncoded, PercentEncoded},
        output::Responder,
    },
    http::{Method, StatusCode},
    std::{marker::PhantomData, path::Path},
};

mod tags {
    #[derive(Debug)]
    pub struct Completed(());

    #[derive(Debug)]
    pub struct Incomplete(());
}

pub fn root() -> Builder<(), self::tags::Incomplete> {
    Builder {
        uri: Uri::root(),
        allowed_methods: None,
        extractor: (),
        _marker: std::marker::PhantomData,
    }
}

pub fn asterisk() -> Builder<(), self::tags::Completed> {
    Builder {
        uri: Uri::asterisk(),
        allowed_methods: Some(AllowedMethods::from(Method::OPTIONS)),
        extractor: (),
        _marker: std::marker::PhantomData,
    }
}

/// A builder of `Scope` to register a route, which is matched to the requests
/// with a certain path and method(s) and will return its response.
#[derive(Debug)]
pub struct Builder<E: Extractor = (), T = self::tags::Incomplete> {
    uri: Uri,
    allowed_methods: Option<AllowedMethods>,
    extractor: E,
    _marker: PhantomData<T>,
}

impl<E> Builder<E, self::tags::Incomplete>
where
    E: Extractor,
{
    /// Sets the HTTP methods that this route accepts.
    ///
    /// By default, the route accepts *all* HTTP methods.
    pub fn allowed_methods(
        self,
        allowed_methods: impl TryInto<AllowedMethods>,
    ) -> super::Result<Self> {
        Ok(Builder {
            allowed_methods: Some(allowed_methods.try_into()?),
            ..self
        })
    }

    /// Appends a *static* segment into this route.
    pub fn segment(mut self, s: impl Into<String>) -> super::Result<Self> {
        self.uri.push(UriComponent::Static(s.into()))?;
        Ok(self)
    }

    /// Appends a trailing slash to the path of this route.
    pub fn slash(self) -> Builder<E, self::tags::Completed> {
        Builder {
            uri: {
                let mut uri = self.uri;
                uri.push(UriComponent::Slash).expect("this is a bug.");
                uri
            },
            allowed_methods: self.allowed_methods,
            extractor: self.extractor,
            _marker: PhantomData,
        }
    }

    /// Appends a parameter with the specified name to the path of this route.
    pub fn param<T>(
        self,
        name: impl Into<String>,
    ) -> super::Result<
        Builder<impl Extractor<Output = <E::Output as Combine<(T,)>>::Out>, self::tags::Incomplete>,
    >
    where
        T: FromPercentEncoded + Send + 'static,
        E::Output: Combine<(T,)> + Send + 'static,
    {
        let name = name.into();
        Ok(Builder {
            uri: {
                let mut uri = self.uri;
                uri.push(UriComponent::Param(name.clone(), ':'))?;
                uri
            },
            allowed_methods: self.allowed_methods,
            extractor: Chain::new(
                self.extractor,
                crate::extractor::ready(move |input| match input.params {
                    Some(ref params) => {
                        let s = params.name(&name).ok_or_else(|| {
                            crate::error::internal_server_error("invalid paramter name")
                        })?;
                        T::from_percent_encoded(unsafe { PercentEncoded::new_unchecked(s) })
                            .map_err(Into::into)
                    }
                    None => Err(crate::error::internal_server_error("missing Params")),
                }),
            ),
            _marker: PhantomData,
        })
    }

    /// Appends a *catch-all* parameter with the specified name to the path of this route.
    pub fn catch_all<T>(
        self,
        name: impl Into<String>,
    ) -> super::Result<
        Builder<impl Extractor<Output = <E::Output as Combine<(T,)>>::Out>, self::tags::Completed>,
    >
    where
        T: FromPercentEncoded + Send + 'static,
        E::Output: Combine<(T,)> + Send + 'static,
    {
        let name = name.into();
        Ok(Builder {
            uri: {
                let mut uri = self.uri;
                uri.push(UriComponent::Param(name.clone(), '*'))?;
                uri
            },
            allowed_methods: self.allowed_methods,
            extractor: Chain::new(
                self.extractor,
                crate::extractor::ready(|input| match input.params {
                    Some(ref params) => {
                        let s = params.catch_all().ok_or_else(|| {
                            crate::error::internal_server_error(
                                "the catch-all parameter is not available",
                            )
                        })?;
                        T::from_percent_encoded(unsafe { PercentEncoded::new_unchecked(s) })
                            .map_err(Into::into)
                    }
                    None => Err(crate::error::internal_server_error("missing Params")),
                }),
            ),
            _marker: PhantomData,
        })
    }
}

impl<E, T> Builder<E, T>
where
    E: Extractor,
{
    /// Appends a supplemental `Extractor` to this route.
    pub fn extract<E2>(self, other: E2) -> Builder<Chain<E, E2>, T>
    where
        E2: Extractor,
        E::Output: Combine<E2::Output> + Send + 'static,
        E2::Output: Send + 'static,
    {
        Builder {
            extractor: Chain::new(self.extractor, other),
            uri: self.uri,
            allowed_methods: self.allowed_methods,
            _marker: PhantomData,
        }
    }

    /// Finalize the configuration in this route and creates the instance of `Route`.
    pub fn to<U>(self, endpoint: U) -> Route<impl Handler<Output = U::Output>>
    where
        U: Endpoint<E::Output> + Clone + Send + 'static,
    {
        let Self {
            uri,
            allowed_methods,
            extractor,
            ..
        } = self;

        let handler = {
            let allowed_methods = allowed_methods.clone();
            crate::handler::raw(move |input| {
                if allowed_methods
                    .as_ref()
                    .map_or(false, |m| !m.contains(input.request.method()))
                {
                    return MaybeFuture::err(StatusCode::METHOD_NOT_ALLOWED.into());
                }

                #[allow(missing_debug_implementations)]
                enum State<F1, F2, E> {
                    First(F1, E),
                    Second(F2),
                }

                let mut state = match extractor.extract(input) {
                    MaybeFuture::Ready(Ok(args)) => match endpoint.call(input, args) {
                        MaybeFuture::Ready(result) => {
                            return MaybeFuture::Ready(result.map_err(Into::into))
                        }
                        MaybeFuture::Future(future) => State::Second(future),
                    },
                    MaybeFuture::Ready(Err(err)) => return MaybeFuture::err(err.into()),
                    MaybeFuture::Future(future) => State::First(future, endpoint.clone()),
                };

                MaybeFuture::Future(crate::future::poll_fn(move |cx| loop {
                    state = match state {
                        State::First(ref mut future, ref endpoint) => {
                            let args =
                                futures01::try_ready!(future.poll_ready(cx).map_err(Into::into));
                            match endpoint.call(&mut *cx.input, args) {
                                MaybeFuture::Ready(result) => {
                                    return result.map(Into::into).map_err(Into::into)
                                }
                                MaybeFuture::Future(future) => State::Second(future),
                            }
                        }
                        State::Second(ref mut future) => {
                            return future.poll_ready(cx).map_err(Into::into)
                        }
                    }
                }))
            })
        };

        Route {
            uri,
            allowed_methods,
            handler,
        }
    }

    /// Creates an instance of `Route` with the current configuration and the specified function.
    ///
    /// The provided function always succeeds and immediately returns a value.
    pub fn reply<F>(self, f: F) -> Route<impl Handler<Output = F::Out>>
    where
        F: Func<E::Output> + Clone + Send + 'static,
        E::Output: 'static,
        F::Out: 'static,
    {
        self.to(crate::endpoint::raw(move |_, args| {
            MaybeFuture::<NeverFuture<_, Never>>::ok(f.call(args))
        }))
    }

    /// Creates an instance of `Route` with the current configuration and the specified function.
    ///
    /// The result of provided function is returned by `Future`.
    pub fn call<F, R>(self, f: F) -> Route<impl Handler<Output = R::Output>>
    where
        F: Func<E::Output, Out = R> + Clone + Send + 'static,
        R: Future + Send + 'static,
        E::Output: 'static,
        F::Out: 'static,
    {
        self.to(crate::endpoint::raw(move |_, args| {
            MaybeFuture::Future(f.call(args))
        }))
    }
}

impl<E, T> Builder<E, T>
where
    E: Extractor<Output = ()>,
{
    /// Creates a `Route` that just replies with the specified `Responder`.
    pub fn say<R>(self, output: R) -> Route<impl Handler<Output = R>>
    where
        R: Clone + Send + 'static,
    {
        self.reply(move || output.clone())
    }

    /// Creates a `Route` that sends the contents of file located at the specified path.
    pub fn send_file(
        self,
        path: impl AsRef<Path>,
        config: Option<crate::fs::OpenConfig>,
    ) -> Route<impl Handler<Output = NamedFile>> {
        let path = crate::fs::ArcPath::from(path.as_ref().to_path_buf());

        self.call(move || {
            crate::future::Compat01::from(match config {
                Some(ref config) => NamedFile::open_with_config(path.clone(), config.clone()),
                None => NamedFile::open(path.clone()),
            })
        })
    }
}

#[derive(Debug)]
pub struct Route<H> {
    uri: Uri,
    allowed_methods: Option<AllowedMethods>,
    handler: H,
}

impl<H> Route<H>
where
    H: Handler,
{
    pub fn new(uri: impl TryInto<Uri>, handler: H) -> super::Result<Self> {
        Ok(Self {
            uri: uri.try_into()?,
            allowed_methods: None,
            handler,
        })
    }
}

impl<H, M> AppConfig<M> for Route<H>
where
    H: Handler,
    M: ModifyHandler<H>,
    M::Output: Responder,
    M::Handler: Send + Sync + 'static,
{
    type Error = super::Error;

    fn configure(self, cx: &mut AppConfigContext<'_, M>) -> Result<(), Self::Error> {
        cx.add_route(self.uri, self.allowed_methods, self.handler)
    }
}
