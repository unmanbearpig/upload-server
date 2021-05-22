#![feature(test)]

extern crate chrono;
extern crate form_urlencoded;

#[macro_use]
extern crate rust_embed;

mod error;
mod sanitize_filename;
mod srv;
mod config;

use std::env;
use config::Config;

fn main() {
    let mut args = env::args();
    args.next();  // skip executable name
    let config = Config::parse_args(&mut args);
    let config = match config {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    let mut srv = config.make_server();

    println!("Listening at {}, upload directiory: {}, name is {}",
             config.listen_addr, config.uploads_dir, config.send_to_name);
    srv.run();
}
