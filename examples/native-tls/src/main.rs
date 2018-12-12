use {
    native_tls::{Identity, TlsAcceptor as NativeTlsAcceptor},
    tokio_tls::TlsAcceptor,
    tsukuyomi::{
        app::config::prelude::*, //
        server::Server,
        App,
    },
};

fn main() -> tsukuyomi::server::Result<()> {
    let der = std::fs::read("./private/identity.p12")?;
    let cert = Identity::from_pkcs12(&der, "mypass")?;
    let acceptor = NativeTlsAcceptor::builder(cert).build()?;
    let acceptor = TlsAcceptor::from(acceptor);

    App::create(
        route().to(endpoint::any() // //
            .say("Hello, Tsukuyomi.\n")),
    ) //
    .map(Server::new)?
    .acceptor(acceptor)
    .run()
}
