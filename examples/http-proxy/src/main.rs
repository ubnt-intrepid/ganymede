#![allow(clippy::needless_pass_by_value)]

mod proxy;

use {
    crate::proxy::Client, //
    futures::prelude::*,
    tsukuyomi::{
        app::config::prelude::*, //
        chain,
        server::Server,
        App,
    },
};

fn main() -> tsukuyomi::server::Result<()> {
    let proxy_client =
        std::sync::Arc::new(crate::proxy::proxy_client(reqwest::r#async::Client::new()));

    App::create(chain![
        route().to(endpoint::any()
            .extract(proxy_client.clone())
            .call(|client: Client| client
                .send_forwarded_request("http://www.example.com")
                .and_then(|resp| resp.receive_all()))),
        route()
            .segment("streaming")?
            .to(endpoint::any()
                .extract(proxy_client)
                .call(|client: Client| client
                    .send_forwarded_request("https://www.rust-lang.org/en-US/"))),
    ]) //
    .map(Server::new)?
    .run()
}
