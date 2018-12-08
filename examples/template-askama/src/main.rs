use {
    askama::Template,
    tsukuyomi::{
        app::{route, App},
        output::Responder,
    },
};

#[derive(Template, Responder)]
#[template(path = "index.html")]
#[responder(respond_to = "tsukuyomi_askama::respond_to")]
struct Index {
    name: String,
}

fn main() -> tsukuyomi::server::Result<()> {
    App::configure(
        route::root() //
            .param("name")?
            .reply(|name| Index { name }),
    )
    .map(tsukuyomi::server::Server::new)?
    .run()
}
