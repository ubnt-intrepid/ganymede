# Tsukuyomi

[![Build Status](https://travis-ci.org/ubnt-intrepid/tsukuyomi.svg?branch=master)](https://travis-ci.org/ubnt-intrepid/tsukuyomi)
[![Build status](https://ci.appveyor.com/api/projects/status/kf8mx9k8iqfa08oj/branch/master?svg=true)](https://ci.appveyor.com/project/ubnt-intrepid/tsukuyomi/branch/master)
[![Coverage Status](https://coveralls.io/repos/github/ubnt-intrepid/tsukuyomi/badge.svg?branch=master)](https://coveralls.io/github/ubnt-intrepid/tsukuyomi?branch=master)
[![Docs.rs](https://docs.rs/tsukuyomi/badge.svg)](https://docs.rs/tsukuyomi)
[![Gitter](https://badges.gitter.im/ubnt-intrepid/tsukuyomi.svg)](https://gitter.im/ubnt-intrepid/tsukuyomi?utm_source=badge&utm_medium=badge&utm_campaign=pr-badge)

Tsukuyomi is a next generation Web framework for Rust.

## The Goal of This Project

The ultimate goal of this project is to provide a Web framework for developing the asynchronous
and fast Web services, with the help of ecosystem of Rust for asynchronous network services like Tokio and Hyper.

## Features

* Supports HTTP/1.x and HTTP/2.0 protocols, based on Hyper 0.12
* Basic support for HTTP/1.1 protocol upgrade
* TLS support by using `rustls`
* Support for both TCP and Unix domain socket
* Custom error handling
* Basic support for Cookie management
* Middleware support

The following features does not currently implemented but will be supported in the future version:

* Custom session storage
* Authentication
* Embedded WebSocket handling

## Example

```rust
extern crate tsukuyomi;

use tsukuyomi::App;

fn main() -> tsukuyomi::AppResult<()> {
    let app = App::builder()
        .mount("/", |r| {
            r.get("/")
                .handle(|_| "Hello, world!\n");
        })
        .finish()?;

    tsukuyomi::run(app);
}
```

More examples are located in [`examples/`](examples/).

## Documentation

* [API documentation (released)](https://docs.rs/tsukuyomi/*/tsukuyomi)
* [API documentation (master)](https://ubnt-intrepid.github.io/tsukuyomi/tsukuyomi/index.html)

## License
MIT + Apache 2.0
