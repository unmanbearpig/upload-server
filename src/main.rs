
extern crate tiny_http;
extern crate form_urlencoded;
extern crate chrono;
extern crate ascii;

use ascii::{AsciiStr, AsciiString};
use std::thread;
use std::time;
use std::process;

use std::fs;
use std::path;
use std::io::{self, Write};

use askama::Template;

#[macro_use]
extern crate rust_embed;


use url::Url;
use std::io::Cursor;

#[derive(RustEmbed)]
#[folder = "assets"]
struct StaticAsset;

#[derive(Template)]
#[template(path = "home.html", escape = "none")]
struct HomeTemplate {
}

#[derive(Template)]
#[template(path = "error.html", escape = "none")]
struct ErrorTemplate<'a> {
    code: u16,
    msg: &'a str,
}

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:2022";

fn content_type_header(value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(
        &b"Content-Type"[..], value).unwrap()
}

struct Srv<'a> {
    http: tiny_http::Server,
    base_url: Url,
    html_content_type: tiny_http::Header,
    die_after_single_request: bool,
    output_path: &'a str,
}

impl<'a> Srv<'a> {
    fn new(http: tiny_http::Server, base_url: Url, output_path: &'a str) -> Self {
        Srv {
            http, base_url,
            html_content_type: content_type_header("text/html"),
            die_after_single_request: false,
            output_path,
        }
    }

    fn die_if_single_request(&self) {
        if self.die_after_single_request {
            // die after a few ms to be restarted by bash script
            //   so we have a new recompiled binary for the next request
            thread::spawn(move || {
                // Give us and the browser some time to fetch assets for the page
                thread::sleep(time::Duration::from_millis(150));

                println!("Handled only one request for debugging. Quitting.");
                process::exit(0);
            });
        }
    }

    fn write_text(&self, text: &str) -> io::Result<()>  {
        let filename: String = {
            let now: chrono::DateTime<chrono::Local> = chrono::offset::Local::now();
            now.format("%F--%T.%f--text.txt").to_string()
        };
        let path = path::Path::new(self.output_path).join(filename);
        let mut file = fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create_new(true)
            .open(path)?;

        let bytes: &[u8] = text.as_bytes();
        file.write_all(bytes)?;
        Ok(())
    }

    fn handle_home(&self, req: tiny_http::Request) {
        let data = HomeTemplate {}.render().unwrap().into_bytes();
        let cur = Cursor::new(data);

        let resp = tiny_http::Response::new(
            tiny_http::StatusCode(200),
            vec![self.html_content_type.clone()],
            cur,
            None, None
        );
        req.respond(resp).expect("error while sending response");
    }

    fn respond_with_error(&self, code: u16, msg: &str, req: tiny_http::Request) {
        let data = ErrorTemplate {
            code,
            msg,
        }.render().unwrap().into_bytes();
        let cur = Cursor::new(data);
        let resp = tiny_http::Response::new(
            tiny_http::StatusCode(code),
            vec![self.html_content_type.clone()],
            cur, None, None
        );
        req.respond(resp).expect("error while sending response");
    }

    fn handle_static_asset(&self, filename: &str, req: tiny_http::Request) {
        match StaticAsset::get(filename) {
            Some(content) => {
                let extension: Option<&str> = filename.split('.').last();

                const DEFAULT_CONTENT_TYPE: &str = "text/plain";
                let content_type: &str = match extension {
                    Some("css") => "text/css",
                    Some("js") => "text/javascript",
                    Some(_) => DEFAULT_CONTENT_TYPE,
                    None => DEFAULT_CONTENT_TYPE,
                };
                let content_type = content_type_header(content_type);
                let cur = Cursor::new(content);

                let resp = tiny_http::Response::new(
                    tiny_http::StatusCode(200),
                    vec![content_type],
                    cur, None, None
                );
                req.respond(resp).expect("error while sending response");
            },
            None => {
                self.respond_with_error(
                    404, format!("Asset \"{}\" not found", filename).as_ref(),
                    req);
            },
        }
    }

    fn handle_text(&self, mut req: tiny_http::Request) {
        if req.method() != &tiny_http::Method::Post {
            self.respond_with_error(
                400, "Send POST to this path", req
            );
            return;
        }

        let mut data = Vec::new();

        match req.as_reader().read_to_end(&mut data) {
            Ok(data) => data,
            Err(err) => {
                self.respond_with_error(500, format!("Error receiving the data: {}", err).as_ref(), req);
                return;
            }
        };

        let mut parser = form_urlencoded::parse(data.as_slice());
        let (k, v) = match parser.next() {
            None => {
                self.respond_with_error(400, "No arguments provided to /text", req);
                return;
            },
            Some(kv) => kv,
        };
        if k != "text" {
            self.respond_with_error(400, format!(
                "Invalid parameter \"{}\" with value \"{}\" ",
                k, v
            ).as_ref(), req);
            return;
        }

        if let Some((k, v)) = parser.next() {
            self.respond_with_error(400, format!(
                "Invalid extra parameter \"{}\" with value \"{}\" ",
                k, v
            ).as_ref(), req);
            return;
        }

        let mut text = v;

        if let Err(msg) = self.write_text(text.to_mut()) {
            self.respond_with_error(
                500, format!("Write error: {}", msg).as_ref(), req
            );
            return;
        }

        self.respond_with_error(
            200, format!("Written text: {}", text).as_ref(), req
        );
    }

    fn save_file_from_request(&self, mut req: tiny_http::Request) -> io::Result<()> {
        let headers = req.headers();
        let mut content_type: Option<AsciiString> = None;
        for header in headers {
            if header.field.as_str() == AsciiStr::from_ascii(b"Content-Type").unwrap() {
                eprintln!("got Content-Type \"{}\"", header.value);
                // TODO: probably could get rid of clone
                content_type = Some(header.value.clone());
            }
        }

        let mut body = Vec::new();
        req.as_reader().read_to_end(&mut body)?;
        println!("body:\n{:?}", body);
    }

    fn handle_file_upload(&self, mut req: tiny_http::Request) {
        match self.save_file_from_request(req) {
            Ok(()) => {
                self.respond_with_error(
                    200, "TODO: File uploaded!", req
                );
            },
            Err(msg) => {
                self.respond_with_error(
                    500, msg.as_ref(), req
                );
            }
        }


    }

    fn handle_request(&self, base_url: &Url, req: tiny_http::Request) {
        self.die_if_single_request();

        let url = req.url();
        if url == "/" {
            return self.handle_home(req);
        }

        let url = base_url.join(url).unwrap();
        let mut path_segments = url.path_segments().unwrap();

        match path_segments.next() {
            Some("assets") => {
                if let Some(filename) = path_segments.next() {
                    self.handle_static_asset(filename, req);
                } else {
                    self.respond_with_error(
                        404, "/assets is not enumeratable",
                        req);

                }
            },
            Some("text") => {
                self.handle_text(req);
            },
            Some("file") => {
                self.handle_file_upload(req);
            },
            Some(other) => {
                self.respond_with_error(
                    404,
                    format!("There's nothing at /{}", other).as_ref(),
                    req);
            },
            None => {
                panic!("first URL path segment is none but we also didn't handle home, \
                        which should never happen. It's a programmer's error.");
            }
        }
    }

    fn run(&mut self) {
        println!("running...");
        loop {
            let req = match self.http.recv() {
                Ok(req) => req,
                Err(e) => {
                    println!("http error: {:?}", e);
                    continue;
                }
            };

            self.handle_request(&self.base_url, req);
        }
    }
}

fn main() {
    let listen_addr = DEFAULT_LISTEN_ADDR;
    let http = match tiny_http::Server::http::<&str>(listen_addr) {
        Ok(http) => http,
        Err(e) => panic!("http start error: {:?}", e),
    };

    let base_url = Url::parse(
        format!("http://{}", listen_addr).as_ref())
        .unwrap();

    let mut srv = Srv::new(http, base_url, "uploads");
    srv.run();

}
