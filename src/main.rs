#![feature(test)]

extern crate chrono;
extern crate form_urlencoded;

#[macro_use]
extern crate rust_embed;

mod error;
mod sanitize_filename;
mod srv;

use url::Url;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:2022";

fn main() {
    let listen_addr = DEFAULT_LISTEN_ADDR;
    let http = match tiny_http::Server::http::<&str>(listen_addr) {
        Ok(http) => http,
        Err(e) => panic!("http start error: {:?}", e),
    };

    let base_url = Url::parse(format!("http://{}", listen_addr).as_ref()).unwrap();

    let mut srv = srv::Srv::new(http, base_url, "uploads");
    srv.run();
}
