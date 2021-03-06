use {
    super::*,
    http::{Response, StatusCode},
    std::borrow::Cow,
};

#[derive(Debug, Clone)]
pub struct Redirect {
    status: StatusCode,
    location: Cow<'static, str>,
}

impl Redirect {
    pub fn new<T>(status: StatusCode, location: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        Self {
            status,
            location: location.into(),
        }
    }
}

impl IntoResponse for Redirect {
    type Body = ();
    type Error = Never;

    #[inline]
    fn into_response(self, _: &Request<()>) -> Result<Response<Self::Body>, Self::Error> {
        Ok(Response::builder()
            .status(self.status)
            .header("location", &*self.location)
            .body(())
            .expect("should be a valid response"))
    }
}

macro_rules! define_funcs {
        ($( $name:ident => $STATUS:ident, )*) => {$(
            #[inline]
            pub fn $name<T>(location: T) -> Redirect
            where
                T: Into<Cow<'static, str>>,
            {
                Redirect::new(StatusCode::$STATUS, location)
            }
        )*};
    }

define_funcs! {
    moved_permanently => MOVED_PERMANENTLY,
    found => FOUND,
    see_other => SEE_OTHER,
    temporary_redirect => TEMPORARY_REDIRECT,
    permanent_redirect => PERMANENT_REDIRECT,
    to => MOVED_PERMANENTLY,
}
