//! Definition of `Modifier` and supplemental components.
//!
//! The purpose of `Modifier` is to insert some processes before and after
//! applying `Handler` in a certain scope.
//!
//! # Examples
//!
//! ```
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use tsukuyomi::{App, Input, Handler};
//! use tsukuyomi::modifier::{Modifier, BeforeHandle, AfterHandle};
//!
//! #[derive(Default)]
//! struct RequestCounter(AtomicUsize);
//!
//! impl Modifier for RequestCounter {
//!     fn before_handle(&self, _: &mut Input) -> BeforeHandle {
//!        self.0.fetch_add(1, Ordering::SeqCst);
//!        BeforeHandle::ok()
//!     }
//! }
//!
//! # fn main() -> tsukuyomi::AppResult<()> {
//! let app = App::builder()
//!     .route(("/", Handler::new_ready(|_| "Hello")))
//!     .modifier(RequestCounter::default())
//!     .finish()?;
//! # Ok(())
//! # }
//! ```

use futures::{self, Future, Poll};
use std::fmt;

use error::Error;
use input::Input;
use output::Output;

/// A trait representing a `Modifier`.
///
/// See the module level documentation for details.
pub trait Modifier {
    /// Performs the process before calling the handler.
    ///
    /// By default, this method does nothing.
    #[allow(unused_variables)]
    fn before_handle(&self, input: &mut Input) -> BeforeHandle {
        BeforeHandle::ok()
    }

    /// Modifies the returned value from a handler.
    ///
    /// By default, this method does nothing and immediately return the provided `Output`.
    #[allow(unused_variables)]
    fn after_handle(&self, input: &mut Input, output: Output) -> AfterHandle {
        AfterHandle::ok(output)
    }
}

// ==== BeforeHandle ====

/// The type representing a return value from `Modifier::before_handle`.
///
/// Roughly speaking, this type is an alias of `Future<Item = Option<Output>, Error = Error>`.
#[derive(Debug)]
pub struct BeforeHandle(BeforeHandleState);

enum BeforeHandleState {
    Ready(Option<Result<(), Error>>),
    Async(Box<dyn Future<Item = (), Error = Error> + Send>),
}

#[cfg_attr(tarpaulin, skip)]
impl fmt::Debug for BeforeHandleState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::BeforeHandleState::*;
        match *self {
            Ready(ref res) => f.debug_tuple("Ready").field(res).finish(),
            Async(..) => f.debug_tuple("Async").finish(),
        }
    }
}

impl BeforeHandle {
    fn ready(res: Result<(), Error>) -> BeforeHandle {
        BeforeHandle(BeforeHandleState::Ready(Some(res)))
    }

    /// Creates an empty value of `BeforeHandle`.
    ///
    /// When this value is received, the framework continues the subsequent processes.
    pub fn ok() -> BeforeHandle {
        BeforeHandle::ready(Ok(()))
    }

    /// Creates a `BeforeHandle` with an error value.
    ///
    /// When this value is received, the framework suspends all remaining processes and immediately
    /// performs the error handling.
    pub fn err<E>(err: E) -> BeforeHandle
    where
        E: Into<Error>,
    {
        BeforeHandle::ready(Err(err.into()))
    }

    /// Creates a `BeforeHandle` from a future.
    pub fn wrap_future<F>(future: F) -> BeforeHandle
    where
        F: Future<Item = (), Error = Error> + Send + 'static,
    {
        BeforeHandle(BeforeHandleState::Async(Box::new(future)))
    }

    pub(crate) fn poll_ready(&mut self, input: &mut Input) -> Poll<(), Error> {
        use self::BeforeHandleState::*;
        match self.0 {
            Ready(ref mut res) => res.take()
                .expect("BeforeHandle has already polled")
                .map(futures::Async::Ready),
            Async(ref mut f) => input.with_set_current(|| f.poll()),
        }
    }
}

// ==== AfterHandle ====

/// The type representing a return value from `Modifier::after_handle`.
#[derive(Debug)]
pub struct AfterHandle(AfterHandleState);

enum AfterHandleState {
    Ready(Option<Result<Output, Error>>),
    Async(Box<dyn Future<Item = Output, Error = Error> + Send>),
}

#[cfg_attr(tarpaulin, skip)]
impl fmt::Debug for AfterHandleState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::AfterHandleState::*;
        match *self {
            Ready(ref res) => f.debug_tuple("Immediate").field(res).finish(),
            Async(..) => f.debug_tuple("Boxed").finish(),
        }
    }
}

impl AfterHandle {
    fn ready(res: Result<Output, Error>) -> AfterHandle {
        AfterHandle(AfterHandleState::Ready(Some(res)))
    }

    /// Creates an `AfterHandle` from an `Output`.
    pub fn ok(output: Output) -> AfterHandle {
        AfterHandle::ready(Ok(output))
    }

    /// Creates an `AfterHandle` from an error value.
    pub fn err<E>(err: E) -> AfterHandle
    where
        E: Into<Error>,
    {
        AfterHandle::ready(Err(err.into()))
    }

    /// Creates an `AfterHandle` from a future.
    pub fn wrap_future<F>(future: F) -> AfterHandle
    where
        F: Future<Item = Output, Error = Error> + Send + 'static,
    {
        AfterHandle(AfterHandleState::Async(Box::new(future)))
    }

    pub(crate) fn poll_ready(&mut self, input: &mut Input) -> Poll<Output, Error> {
        use self::AfterHandleState::*;
        match self.0 {
            Ready(ref mut res) => res.take()
                .expect("AfterHandle has already polled")
                .map(futures::Async::Ready),
            Async(ref mut f) => input.with_set_current(|| f.poll()),
        }
    }
}
