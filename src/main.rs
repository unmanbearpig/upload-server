#![feature(test)]

// TODO: add some kind of user agent to the filename

extern crate form_urlencoded;
extern crate chrono;
extern crate ascii;

// extern crate multipart;
use multipart::server::{Multipart, SaveResult};

use std::thread;
use std::time;
use std::process;

use std::fs;
use std::path;
use std::io::{self, Write};
use std::fmt;

use askama::Template;

#[macro_use]
extern crate rust_embed;

use url::Url;
use std::io::Cursor;

mod sanitize_filename;
use sanitize_filename::sanitize_filename;

#[derive(RustEmbed)]
#[folder = "assets"]
struct StaticAsset;

#[derive(Template)]
#[template(path = "home.html", escape = "none")]
struct HomeTemplate {
}

#[derive(Template)]
#[template(path = "error_page.html", escape = "none")]
struct ErrorPageTemplate<'a> {
    err: &'a Error
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

#[derive(Copy, Clone, Debug)]
enum ErrorKind {
    Success,
    ServerError,
    UserError,
    NotFound,
    Unknown,
}

impl ErrorKind {
    fn as_http_code(self) -> u16 {
        match self {
            ErrorKind::Success => 200,
            ErrorKind::ServerError => 500,
            ErrorKind::UserError => 400,
            ErrorKind::NotFound => 404,
            ErrorKind::Unknown => 500,
        }
    }

    fn description(self) -> &'static str {
        match self {
            ErrorKind::Success => "Success",
            ErrorKind::ServerError => "Server error",
            ErrorKind::UserError => "Client error",
            ErrorKind::NotFound => "Not found",
            ErrorKind::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[derive(Debug, Clone, Copy)]
enum UploadType {
    Text,
    File,
}

impl UploadType {
    fn name(self) -> &'static str {
        match self {
            UploadType::Text => "text",
            UploadType::File => "file",
        }
    }

    fn as_file_suffix(self) -> &'static str {
        match self {
            UploadType::Text => "text.txt",
            UploadType::File => "file.bin",
        }
    }
}

impl fmt::Display for UploadType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Template, Debug)]
#[template(path = "error.html", escape = "none")] // TODO: is escape = none safe here?
struct Error {
    kind: ErrorKind,
    msg: String,
}

impl Error {
    fn new<T: fmt::Display>(kind: ErrorKind, msg: T) -> Self {
        Error {
            kind, msg: msg.to_string(),
        }
    }

    fn from_io_error<T: AsRef<str>>(err: io::Error, description: T) -> Self {
        Error {
            kind: ErrorKind::ServerError,
            msg: format!("{}: {}", description.as_ref(), err),
        }
    }

    fn as_http_code(&self) -> u16 {
        self.kind.as_http_code()
    }
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

    fn create_file<T: AsRef<str>>(&self, typ: UploadType, name: Option<T>) -> io::Result<fs::File> {
        let date_str = {
            let now: chrono::DateTime<chrono::Local> = chrono::offset::Local::now();
            now.format("%F--%T.%f").to_string()
        };


        let filename = match name {
            Some(name) => format!("{}--{}--{}", date_str, name.as_ref(), typ.as_file_suffix()),
            None => format!("{}--{}", date_str, typ.as_file_suffix()),
        };

        let path = path::Path::new(self.output_path).join(filename);
        fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create_new(true)
            .open(path)
    }

    fn write_text(&self, text: &str) -> io::Result<()>  {
        let mut file = self.create_file::<&str>(UploadType::Text, None)?;
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

    fn respond_with_error(&self, err: &Error, req: tiny_http::Request) {
        let data = ErrorPageTemplate {
            err
        }.render().unwrap().into_bytes();
        let cur = Cursor::new(data);
        let resp = tiny_http::Response::new(
            tiny_http::StatusCode(err.as_http_code()),
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
                    &Error::new(
                        ErrorKind::NotFound,
                        format!("Asset \"{}\" not found", filename)),
                    req);
            },
        }
    }

    fn save_text(&self, req: &mut tiny_http::Request) -> Result<String, Error> {
        if req.method() != &tiny_http::Method::Post {
            return Err(
                Error::new(ErrorKind::UserError, "Send POST to this path"),
            );
        }

        let mut data = Vec::new();

        req.as_reader().read_to_end(&mut data)
            .map_err( |e| Error::from_io_error(
                e, "Error receiving the data"))?;

        let mut parser = form_urlencoded::parse(data.as_slice());
        let (k, v) = match parser.next() {
            None => {
                return Err(Error::new(
                    ErrorKind::UserError, "No arguments provided to /text")
                );
            },
            Some(kv) => kv,
        };
        if k != "text" {
            return Err(Error::new(
                ErrorKind::UserError,
                format!(
                    "Invalid parameter \"{}\" with value \"{}\" ",
                    k, v)
            ));
        }

        if let Some((k, v)) = parser.next() {
            return Err(Error::new(
                ErrorKind::UserError,
                format!(
                    "Invalid extra parameter \"{}\" with value \"{}\" ",
                    k, v
                )
            ));
        }

        let mut text = v;

        self.write_text(text.to_mut())
            .map_err( |e| Error::from_io_error(e, "Write error"))?;

        Ok(format!("Saved text: {}", text))
    }

    fn handle_text(&self, mut req: tiny_http::Request) {
        match self.save_text(&mut req) {
            Ok(msg) => {
                self.respond_with_error(&Error::new(
                    ErrorKind::Success, msg
                ), req)
            },
            Err(err) => {
                dbg!(err);
                todo!()
            }
        }
    }

    fn save_file_from_request(&self, req: &mut tiny_http::Request) -> Result<(), Error> {
        let mut req = Multipart::from_request(req)
            .map_err(
                |e| Error::new(ErrorKind::ServerError, format!("{:?}", e)
                )
            )?;

        let mut err: Result<(), Error> =
            Err(Error::new(ErrorKind::UserError, "no entries provided"));
        req.foreach_entry(|mut entry| {
            let name = &*entry.headers.name.clone();
            if name == "file" {
                // read file here
                let file = self.create_file(
                    UploadType::File,
                    entry.headers.filename.map(sanitize_filename))
                    .map_err(|e| Error::from_io_error(e, "create file error"));

                let file = match file {
                    Ok(file) => file,
                    Err(e) => {
                        err = Err(e);
                        return;
                    },
                };

                let result = entry.data.save().memory_threshold(64*1024*1024).write_to(file);

                match result {
                    SaveResult::Full(_) => {},
                    SaveResult::Partial(partial, partial_reason) => {
                        err = Err(Error::new(
                            ErrorKind::Unknown,
                            format!("data partially saved/received, partial = {}, partial_reason = {:?}",
                                    partial, partial_reason)))
                    },
                    SaveResult::Error(error) => {
                        err = Err(Error::new(
                            ErrorKind::ServerError,
                            format!("data save error: {}", error)));
                    }
                }
            } else {
                err = Err(Error::new(
                    ErrorKind::UserError, format!("invalid entry (expected only \"file\") {}", name)));
            }
        }).map_err(|e| Error::new(
            ErrorKind::ServerError,
            format!("foreach_entry error: {:?}", e)))?;

        Ok(())
    }

    fn handle_file_upload(&self, mut req: tiny_http::Request) {
        match self.save_file_from_request(&mut req) {
            Ok(()) => {
                self.respond_with_error(
                    &Error::new(ErrorKind::Success, "TODO: File uploaded!"),
                    req
                );
            },
            Err(err) => {
                self.respond_with_error(&err, req);
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
                        &Error::new(ErrorKind::NotFound, "/assets is not enumeratable"),
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
                    &Error::new(ErrorKind::NotFound,
                                format!("There's nothing at /{}", other)),
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