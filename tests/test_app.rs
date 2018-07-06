extern crate cookie;
extern crate futures;
extern crate http;
extern crate time;
extern crate tsukuyomi;

use tsukuyomi::handler::Handler;
use tsukuyomi::local::LocalServer;
use tsukuyomi::{App, Input};

use futures::future::lazy;
use http::{header, StatusCode};

#[test]
fn test_case1_empty_routes() {
    let app = App::builder().finish().unwrap();
    let mut server = LocalServer::new(app).unwrap();

    let response = server.client().get("/").execute().unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn test_case2_single_route() {
    let app = App::builder()
        .mount("/", |m| {
            m.get("/hello").handle(Handler::new_ready(|_| "Tsukuyomi"));
        })
        .finish()
        .unwrap();
    let mut server = LocalServer::new(app).unwrap();

    let response = server.client().get("/hello").execute().unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"text/plain; charset=utf-8"[..])
    );
    assert_eq!(
        response.headers().get(header::CONTENT_LENGTH).map(|v| v.as_bytes()),
        Some(&b"9"[..])
    );
    assert_eq!(*response.body().to_bytes(), b"Tsukuyomi"[..]);
}

#[test]
fn test_case3_post_body() {
    let app = App::builder()
        .mount("/", |m| {
            m.post("/hello").handle(Handler::new_fully_async(|| {
                lazy(|| {
                    let read_all = Input::with_current(|input| input.body_mut().read_all());
                    read_all.convert_to::<String>()
                })
            }));
        })
        .finish()
        .unwrap();
    let mut server = LocalServer::new(app).unwrap();

    let response = server
        .client()
        .post("/hello")
        .body("Hello, Tsukuyomi.")
        .execute()
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).map(|v| v.as_bytes()),
        Some(&b"text/plain; charset=utf-8"[..])
    );
    assert_eq!(
        response.headers().get(header::CONTENT_LENGTH).map(|v| v.as_bytes()),
        Some(&b"17"[..])
    );
    assert_eq!(*response.body().to_bytes(), b"Hello, Tsukuyomi."[..]);
}

#[test]
fn test_case4_cookie() {
    use cookie::Cookie;
    use time::Duration;

    let expires_in = time::now() + Duration::days(7);

    let app = App::builder()
        .mount("/", |m| {
            m.get("/login").handle(Handler::new_ready({
                move |input| -> tsukuyomi::Result<_> {
                    #[cfg_attr(rustfmt, rustfmt_skip)]
                    let cookie = Cookie::build("session", "dummy_session_id")
                        .domain("www.example.com")
                        .expires(expires_in)
                        .finish();
                    input.cookies()?.add(cookie);
                    Ok("Logged in")
                }
            }));

            m.get("/logout")
                .handle(Handler::new_ready(move |input| -> tsukuyomi::Result<_> {
                    input.cookies()?.remove(Cookie::named("session"));
                    Ok("Logged out")
                }));
        })
        .finish()
        .unwrap();

    let mut server = LocalServer::new(app).unwrap();

    let response = server.client().get("/login").execute().unwrap();
    assert!(response.headers().contains_key(header::SET_COOKIE));

    let cookie_str = response.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
    let cookie = Cookie::parse_encoded(cookie_str).unwrap();

    assert_eq!(cookie.name(), "session");
    assert_eq!(cookie.domain(), Some("www.example.com"));
    assert_eq!(
        cookie.expires().map(|tm| tm.to_timespec().sec),
        Some(expires_in.to_timespec().sec)
    );

    let response = server
        .client()
        .get("/logout")
        .header(header::COOKIE, cookie_str)
        .execute()
        .unwrap();
    assert!(response.headers().contains_key(header::SET_COOKIE));

    let cookie_str = response.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
    let cookie = Cookie::parse_encoded(cookie_str).unwrap();

    assert_eq!(cookie.name(), "session");
    assert_eq!(cookie.value(), "");
    assert_eq!(cookie.max_age(), Some(Duration::zero()));
    assert!(cookie.expires().map_or(false, |tm| tm < time::now()));

    let response = server.client().get("/logout").execute().unwrap();
    assert!(!response.headers().contains_key(header::SET_COOKIE));
}