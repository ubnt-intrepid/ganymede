use {
    super::{AppData, EndpointData, EndpointId, Recognize, ScopeId},
    cookie::{Cookie, CookieJar},
    crate::{
        error::{Critical, Error},
        handler::AsyncResult,
        input::RequestBody,
        localmap::LocalMap,
        output::{Output, ResponseBody},
        recognizer::Captures,
        uri::CaptureNames,
    },
    futures::{Async, Future, IntoFuture, Poll},
    http::{
        header::{self, HeaderMap, HeaderValue},
        Method, Request, Response, StatusCode,
    },
    hyper::body::Payload,
    mime::Mime,
    std::{marker::PhantomData, mem, ops::Index, rc::Rc, sync::Arc},
};

macro_rules! ready {
    ($e:expr) => {
        match $e {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(x)) => Ok(x),
            Err(e) => Err(e),
        }
    };
}

/// A future for managing an incoming HTTP request, created by `AppService`.
#[must_use = "futures do nothing unless polled"]
#[derive(Debug)]
pub struct AppFuture {
    request: Request<()>,
    data: Arc<AppData>,
    body: BodyState,
    cookie_jar: Option<CookieJar>,
    response_headers: Option<HeaderMap>,
    locals: LocalMap,
    endpoint_id: Option<EndpointId>,
    captures: Option<Captures>,
    state: AppFutureState,
}

#[cfg_attr(feature = "cargo-clippy", allow(large_enum_variant))]
#[derive(Debug)]
enum AppFutureState {
    Init,
    InFlight(AsyncResult<Output>),
    Done,
}

#[derive(Debug)]
enum BodyState {
    Some(RequestBody),
    Gone,
    Upgraded,
}

macro_rules! input {
    ($self:expr) => {
        &mut Input {
            request: &$self.request,
            params: {
                &if let Some(endpoint_id) = $self.endpoint_id {
                    if let (Some(names), &Some(ref captures)) =
                        ($self.data.uri(endpoint_id).capture_names(), &$self.captures)
                    {
                        Some(Params {
                            path: $self.request.uri().path(),
                            names,
                            captures,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            },
            states: &States {
                data: &$self.data,
                scope_id: $self.endpoint_id.map(|EndpointId(scope, _)| scope),
            },
            cookies: &mut Cookies {
                jar: &mut $self.cookie_jar,
                request_headers: &$self.request.headers(),
                _marker: PhantomData,
            },
            locals: &mut $self.locals,
            body: &mut $self.body,
            response_headers: &mut $self.response_headers,
            data: &*$self.data,
            endpoint_id: $self.endpoint_id,
            _marker: PhantomData,
        }
    };
}

impl AppFuture {
    pub(super) fn new(request: Request<RequestBody>, data: Arc<AppData>) -> Self {
        let (parts, body) = request.into_parts();
        Self {
            request: Request::from_parts(parts, ()),
            data,
            body: BodyState::Some(body),
            cookie_jar: None,
            response_headers: None,
            locals: LocalMap::default(),
            endpoint_id: None,
            captures: None,
            state: AppFutureState::Init,
        }
    }

    fn handle_fallback(&self, endpoint: &EndpointData) -> AsyncResult<Output> {
        let allowed_methods = endpoint.allowed_methods_value.clone();
        AsyncResult::ready(move |input| {
            if input.request.method() == Method::OPTIONS {
                let mut response = Response::new(ResponseBody::default());
                response
                    .headers_mut()
                    .insert(http::header::ALLOW, allowed_methods);
                Ok(response)
            } else {
                Err(StatusCode::METHOD_NOT_ALLOWED.into())
            }
        })
    }

    fn apply_all_modifiers(
        &self,
        mut in_flight: AsyncResult<Output>,
        id: ScopeId,
    ) -> AsyncResult<Output> {
        let scope = self.data.scope(id);
        for modifier in scope.modifiers.iter().rev() {
            in_flight = modifier.modify(in_flight);
        }
        for &parent in scope.parents.iter().rev() {
            let scope = self.data.scope(parent);
            for modifier in scope.modifiers.iter().rev() {
                in_flight = modifier.modify(in_flight);
            }
        }
        in_flight
    }

    fn apply_global_modifiers(&self, mut in_flight: AsyncResult<Output>) -> AsyncResult<Output> {
        for modifier in self.data.global_scope.modifiers.iter().rev() {
            in_flight = modifier.modify(in_flight);
        }
        in_flight
    }

    fn process_recognize(&mut self) -> AsyncResult<Output> {
        match self
            .data
            .recognize(self.request.uri().path(), self.request.method())
        {
            Recognize::Matched {
                route,
                endpoint,
                captures,
                ..
            } => {
                self.endpoint_id = Some(endpoint.id);
                self.captures = captures;
                self.apply_all_modifiers(route.handler.handle(), endpoint.id.0)
            }

            Recognize::MethodNotAllowed {
                endpoint, captures, ..
            } => {
                self.endpoint_id = Some(endpoint.id);
                self.captures = captures;
                self.apply_all_modifiers(self.handle_fallback(endpoint), endpoint.id.0)
            }

            Recognize::NotFound => {
                self.apply_global_modifiers(AsyncResult::err(StatusCode::NOT_FOUND.into()))
            }
        }
    }

    fn process_on_error(&mut self, err: Error) -> Result<Output, Critical> {
        self.data.on_error.call(err, input!(self))
    }

    fn process_before_reply(&mut self, output: &mut Output) {
        // append Cookie entries.
        if let Some(ref jar) = self.cookie_jar {
            for cookie in jar.delta() {
                output.headers_mut().append(
                    header::SET_COOKIE,
                    cookie.encoded().to_string().parse().unwrap(),
                );
            }
        }

        // append supplemental response headers.
        if let Some(mut hdrs) = self.response_headers.take() {
            for (k, v) in hdrs.drain() {
                output.headers_mut().extend(v.map(|v| (k.clone(), v)));
            }
        }

        // append the value of Content-Length to the response header if missing.
        if let Some(len) = output.body().content_length() {
            output
                .headers_mut()
                .entry(header::CONTENT_LENGTH)
                .expect("never fails")
                .or_insert_with(|| {
                    // safety: '0'-'9' is ascii.
                    // TODO: more efficient
                    unsafe { HeaderValue::from_shared_unchecked(len.to_string().into()) }
                });
        }
    }
}

impl Future for AppFuture {
    type Item = Response<ResponseBody>;
    type Error = Critical;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let polled = loop {
            self.state = match self.state {
                AppFutureState::Init => AppFutureState::InFlight(self.process_recognize()),
                AppFutureState::InFlight(ref mut in_flight) => {
                    break ready!(in_flight.poll_ready(input!(self)))
                }
                AppFutureState::Done => panic!("the future has already polled."),
            };
        };

        let mut output = match polled {
            Ok(output) => output,
            Err(err) => {
                self.state = AppFutureState::Done;
                self.process_on_error(err)?
            }
        };

        self.process_before_reply(&mut output);

        Ok(Async::Ready(output))
    }
}

#[derive(Debug)]
pub struct States<'task> {
    data: &'task Arc<AppData>,
    scope_id: Option<ScopeId>,
}

impl<'task> States<'task> {
    /// Returns the reference to a shared state of `T` registered in the scope.
    #[inline]
    pub fn try_get<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.data.get_state(self.scope_id?)
    }

    /// Returns the reference to a shared state of `T` registered in the scope.
    ///
    /// # Panics
    /// This method will panic if the state of `T` is not registered in the scope.
    #[inline]
    pub fn get<T>(&self) -> &T
    where
        T: Send + Sync + 'static,
    {
        self.try_get().expect("The state is not set")
    }
}

/// A proxy object for accessing Cookie values.
#[derive(Debug)]
pub struct Cookies<'task> {
    jar: &'task mut Option<CookieJar>,
    request_headers: &'task HeaderMap,
    _marker: PhantomData<Rc<()>>,
}

impl<'task> Cookies<'task> {
    /// Returns the mutable reference to the inner `CookieJar` if available.
    pub fn jar(&mut self) -> crate::error::Result<&mut CookieJar> {
        if let Some(ref mut jar) = self.jar {
            return Ok(jar);
        }

        let jar = self.jar.get_or_insert_with(CookieJar::new);

        for raw in self.request_headers.get_all(http::header::COOKIE) {
            let raw_s = raw.to_str().map_err(crate::error::bad_request)?;
            for s in raw_s.split(';').map(|s| s.trim()) {
                let cookie = Cookie::parse_encoded(s)
                    .map_err(crate::error::bad_request)?
                    .into_owned();
                jar.add_original(cookie);
            }
        }

        Ok(jar)
    }
}

#[cfg(feature = "secure")]
mod secure {
    use cookie::{Key, PrivateJar, SignedJar};
    use crate::error::Result;

    impl<'a> super::Cookies<'a> {
        /// Creates a `SignedJar` with the specified secret key.
        #[inline]
        pub fn signed_jar(&mut self, key: &Key) -> Result<SignedJar<'_>> {
            Ok(self.jar()?.signed(key))
        }

        /// Creates a `PrivateJar` with the specified secret key.
        #[inline]
        pub fn private_jar(&mut self, key: &Key) -> Result<PrivateJar<'_>> {
            Ok(self.jar()?.private(key))
        }
    }
}

/// A proxy object for accessing extracted parameters.
#[derive(Debug)]
pub struct Params<'input> {
    path: &'input str,
    names: &'input CaptureNames,
    captures: &'input Captures,
}

impl<'input> Params<'input> {
    /// Returns `true` if the extracted paramater exists.
    pub fn is_empty(&self) -> bool {
        self.captures.params().is_empty() && self.captures.wildcard().is_none()
    }

    /// Returns the value of `i`-th parameter, if exists.
    pub fn get(&self, i: usize) -> Option<&str> {
        let &(s, e) = self.captures.params().get(i)?;
        self.path.get(s..e)
    }

    /// Returns the value of wildcard parameter, if exists.
    pub fn get_wildcard(&self) -> Option<&str> {
        let (s, e) = self.captures.wildcard()?;
        self.path.get(s..e)
    }

    /// Returns the value of parameter whose name is equal to `name`, if exists.
    pub fn name(&self, name: &str) -> Option<&str> {
        match name {
            "*" => self.get_wildcard(),
            name => self.get(self.names.position(name)?),
        }
    }
}

impl<'input> Index<usize> for Params<'input> {
    type Output = str;

    fn index(&self, i: usize) -> &Self::Output {
        self.get(i).expect("Out of range")
    }
}

impl<'input, 'a> Index<&'a str> for Params<'input> {
    type Output = str;

    fn index(&self, name: &'a str) -> &Self::Output {
        self.name(name).expect("Out of range")
    }
}

/// A proxy object for accessing the contextual information about incoming HTTP request
/// and global/request-local state.
#[derive(Debug)]
pub struct Input<'task> {
    /// The information of incoming request without the message body.
    pub request: &'task Request<()>,

    /// A set of extracted parameters from router.
    pub params: &'task Option<Params<'task>>,

    /// A proxy object for accessing shared states.
    pub states: &'task States<'task>,

    /// A proxy object for accessing Cookie values.
    pub cookies: &'task mut Cookies<'task>,

    /// A typemap that holds arbitrary request-local data.
    pub locals: &'task mut LocalMap,

    body: &'task mut BodyState,
    response_headers: &'task mut Option<HeaderMap>,
    data: &'task AppData,
    endpoint_id: Option<EndpointId>,
    _marker: PhantomData<Rc<()>>,
}

impl<'task> Input<'task> {
    /// Takes a raw instance of incoming message body from the context.
    pub fn body(&mut self) -> Option<RequestBody> {
        match mem::replace(self.body, BodyState::Gone) {
            BodyState::Some(body) => Some(body),
            _ => None,
        }
    }

    /// Registers the upgrade handler to the context.
    #[inline]
    pub fn upgrade<F, R>(&mut self, on_upgrade: F) -> Result<(), F>
    where
        F: FnOnce(crate::input::body::UpgradedIo) -> R + Send + 'static,
        R: IntoFuture<Item = (), Error = ()>,
        R::Future: Send + 'static,
    {
        let body = match mem::replace(self.body, BodyState::Upgraded) {
            BodyState::Some(body) => body,
            _ => return Err(on_upgrade),
        };

        crate::rt::spawn(
            body.on_upgrade()
                .map_err(|_| ())
                .and_then(move |upgraded| on_upgrade(upgraded).into_future()),
        );

        Ok(())
    }

    /// Returns 'true' if the context has already upgraded.
    pub fn is_upgraded(&self) -> bool {
        match self.body {
            BodyState::Upgraded => true,
            _ => false,
        }
    }

    /// Parses the header field `Content-type` and stores it into the localmap.
    pub fn content_type(&mut self) -> Result<Option<&Mime>, Error> {
        use crate::localmap::{local_key, Entry};

        local_key! {
            static KEY: Option<Mime>;
        }

        match self.locals.entry(&KEY) {
            Entry::Occupied(entry) => Ok(entry.into_mut().as_ref()),
            Entry::Vacant(entry) => {
                let mime = match self.request.headers().get(http::header::CONTENT_TYPE) {
                    Some(h) => h
                        .to_str()
                        .map_err(crate::error::bad_request)?
                        .parse()
                        .map(Some)
                        .map_err(crate::error::bad_request)?,
                    None => None,
                };
                Ok(entry.insert(mime).as_ref())
            }
        }
    }

    pub fn response_headers(&mut self) -> &mut HeaderMap {
        self.response_headers.get_or_insert_with(Default::default)
    }

    pub fn allowed_methods<'a>(&'a self) -> Option<impl Iterator<Item = &'a Method> + 'a> {
        Some(
            self.data
                .endpoints
                .get_index(self.endpoint_id?.1)?
                .1
                .route_ids
                .keys(),
        )
    }
}
