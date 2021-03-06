use {
    futures::prelude::*,
    tsukuyomi::{
        config::prelude::*, //
        fs::Staticfiles,
        output::redirect,
        App,
    },
    tsukuyomi_server::Server,
    tsukuyomi_tungstenite::{Message, Ws},
};

const STATIC_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/static");

fn main() -> tsukuyomi_server::Result<()> {
    App::create(chain![
        path!("/ws") //
            .to(endpoint::get().reply(Ws::new(|stream| {
                let (tx, rx) = stream.split();
                rx.filter_map(|m| {
                    println!("Message from client: {:?}", m);
                    match m {
                        Message::Ping(p) => Some(Message::Pong(p)),
                        Message::Pong(_) => None,
                        _ => Some(m),
                    }
                }) //
                .forward(tx)
                .then(|_| Ok(()))
            }))),
        path!("/") //
            .to(endpoint::reply(redirect::to("/index.html"))),
        Staticfiles::new(STATIC_PATH)
    ]) //
    .map(Server::new)?
    .run()
}
