extern crate tsukuyomi;
extern crate tsukuyomi_fs;

use tsukuyomi::app::{route, scope, App};
use tsukuyomi_fs::Staticfiles;

fn main() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let app = App::builder()
        .route(
            route!("/") //
                .serve_file(manifest_dir.join("/static/index.html")),
        ) //
        .mount(
            scope::builder() //
                .with(Staticfiles::new(manifest_dir.join("static"))),
        ) //
        .finish()
        .unwrap();

    tsukuyomi::server(app) //
        .run_forever()
        .unwrap();
}
