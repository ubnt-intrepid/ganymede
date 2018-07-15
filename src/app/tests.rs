use super::router::{Recognize, RecognizeErrorKind};
use super::*;
use handler::Handler;
use http::Method;

#[test]
fn empty() {
    let app = App::builder().finish().unwrap();
    assert_matches!(
        app.router().recognize("/", &Method::GET),
        Err(RecognizeErrorKind::NotFound)
    );
}

#[test]
fn root_single_method() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/").handle(Handler::new_ready(|_| "a"));
        })
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/", &Method::GET),
        Ok(Recognize { endpoint_id: 0, .. })
    );

    assert_matches!(
        app.router().recognize("/path/to", &Method::GET),
        Err(RecognizeErrorKind::NotFound)
    );
    assert_matches!(
        app.router().recognize("/", &Method::POST),
        Err(RecognizeErrorKind::MethodNotAllowed)
    );
}

#[test]
fn root_multiple_method() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/").handle(Handler::new_ready(|_| "a"));
            m.post("/").handle(Handler::new_ready(|_| "b"));
        })
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/", &Method::GET),
        Ok(Recognize { endpoint_id: 0, .. })
    );
    assert_matches!(
        app.router().recognize("/", &Method::POST),
        Ok(Recognize { endpoint_id: 1, .. })
    );

    assert_matches!(
        app.router().recognize("/", &Method::PUT),
        Err(RecognizeErrorKind::MethodNotAllowed)
    );
}

#[test]
fn root_fallback_head() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/").handle(Handler::new_ready(|_| "a"));
        })
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/", &Method::HEAD),
        Ok(Recognize { endpoint_id: 0, .. })
    );
}

#[test]
fn root_fallback_head_disabled() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/").handle(Handler::new_ready(|_| "a"));
        })
        .fallback_head(false)
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/", &Method::HEAD),
        Err(RecognizeErrorKind::MethodNotAllowed)
    );
}

#[test]
fn fallback_options() {
    let app = App::builder()
        .mount("/path/to", |m| {
            m.get("/foo").handle(Handler::new_ready(|_| "a"));
            m.post("/foo").handle(Handler::new_ready(|_| "b"));
        })
        .fallback_options(true)
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/path/to/foo", &Method::OPTIONS),
        Err(RecognizeErrorKind::FallbackOptions { .. })
    );
}

#[test]
fn fallback_options_disabled() {
    let app = App::builder()
        .mount("/path/to", |m| {
            m.get("/foo").handle(Handler::new_ready(|_| "a"));
            m.post("/foo").handle(Handler::new_ready(|_| "b"));
        })
        .fallback_options(false)
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/path/to/foo", &Method::OPTIONS),
        Err(RecognizeErrorKind::MethodNotAllowed)
    );
}

#[test]
fn mount() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/foo").handle(Handler::new_ready(|_| "a")); // /foo
            m.get("/bar").handle(Handler::new_ready(|_| "b")); // /bar
        })
        .mount("/baz", |m| {
            m.get("/").handle(Handler::new_ready(|_| "c")); // /baz

            m.mount("/", |m| {
                m.get("/").handle(Handler::new_ready(|_| "d")); // /baz
                m.get("/foobar").handle(Handler::new_ready(|_| "e")); // /baz/foobar
            });
        })
        .finish()
        .unwrap();

    assert_matches!(
        app.router().recognize("/foo", &Method::GET),
        Ok(Recognize { endpoint_id: 0, .. })
    );
    assert_matches!(
        app.router().recognize("/bar", &Method::GET),
        Ok(Recognize { endpoint_id: 1, .. })
    );
    assert_matches!(
        app.router().recognize("/baz", &Method::GET),
        Ok(Recognize { endpoint_id: 3, .. })
    );
    assert_matches!(
        app.router().recognize("/baz/foobar", &Method::GET),
        Ok(Recognize { endpoint_id: 4, .. })
    );

    assert_matches!(
        app.router().recognize("/baz/", &Method::GET),
        Err(RecognizeErrorKind::NotFound)
    );
}

#[test]
fn scope_variable() {
    let app = App::builder()
        .manage::<String>("G".into())
        .mount("/s0", |m| {
            m.mount("/s1", |m| {
                m.set::<String>("A".into());
            });
        })
        .mount("/s2", |m| {
            m.set::<String>("B".into());
            m.mount("/s3", |m| {
                m.set::<String>("C".into());
                m.mount("/s4", |_m| {});
            }).mount("/s5", |m| {
                m.mount("/s6", |_m| {});
            });
        })
        .finish()
        .unwrap();

    {
        let inner_string = app.states().get_inner::<String>().unwrap();

        assert_eq!(inner_string.global, Some("G".into()));
        assert_eq!(
            inner_string.locals,
            vec![
                None,
                Some("A".into()),
                Some("B".into()),
                Some("C".into()),
                None,
                None,
                None,
            ]
        );
    }

    assert_eq!(app.states().get(0).map(String::as_str), Some("G"));
    assert_eq!(app.states().get(1).map(String::as_str), Some("A"));
    assert_eq!(app.states().get(2).map(String::as_str), Some("B"));
    assert_eq!(app.states().get(3).map(String::as_str), Some("C"));
    assert_eq!(app.states().get(4).map(String::as_str), Some("C"));
    assert_eq!(app.states().get(5).map(String::as_str), Some("B"));
    assert_eq!(app.states().get(6).map(String::as_str), Some("B"));
}
