use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path;

use std::io::Cursor;
use url::Url;

use std::process;
use std::thread;
use std::time;

use multipart::server::{Multipart, SaveResult};

use crate::sanitize_filename::sanitize_filename;

#[derive(RustEmbed)]
#[folder = "assets"]
struct StaticAsset;

type BinaryResponse = tiny_http::Response<Cursor<Vec<u8>>>;

use crate::error::{Error, ErrorKind};

fn content_type_header(value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], value).unwrap()
}

pub struct Srv<'a> {
    http: tiny_http::Server,
    base_url: Url,
    html_content_type: tiny_http::Header,
    die_after_single_request: bool,
    output_path: &'a str,
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

impl<'a> Srv<'a> {
    pub fn new(http: tiny_http::Server, base_url: Url, output_path: &'a str)
               -> Self {
        Srv {
            http,
            base_url,
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
                // Give us and the browser some time to fetch assets
                // for the page
                thread::sleep(time::Duration::from_millis(150));

                println!("Handled only one request for debugging. Quitting.");
                process::exit(0);
            });
        }
    }

    fn create_file<T: AsRef<str>>(&self, typ: UploadType, name: Option<T>)
                                  -> io::Result<fs::File> {
        let date_str = {
            let now: chrono::DateTime<chrono::Local> =
                chrono::offset::Local::now();
            now.format("%F--%T.%f").to_string()
        };

        let filename = match name {
            Some(name) => format!(
                "{}--{}--{}",
                date_str, name.as_ref(), typ.as_file_suffix()),
            None => format!("{}--{}", date_str, typ.as_file_suffix()),
        };

        let path = path::Path::new(self.output_path).join(filename);
        fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create_new(true)
            .open(path)
    }

    fn write_text(&self, text: &str) -> io::Result<()> {
        let mut file = self.create_file::<&str>(UploadType::Text, None)?;
        let bytes: &[u8] = text.as_bytes();
        file.write_all(bytes)?;
        Ok(())
    }

    fn handle_home(&self) -> Result<BinaryResponse, Error> {
        self.handle_static_asset("home.html")
    }

    fn error_response(&self, err: &Error) -> BinaryResponse {
        let data = format!(
            r#"
<html>
  <body style="font-size: 48px">
    {}

    <br/>
    <a href="/">Go back</a>
  </body>
</html>

"#, err.as_html()).into_bytes();
        let cur = Cursor::new(data);
        tiny_http::Response::new(
            tiny_http::StatusCode(err.as_http_code()),
            vec![self.html_content_type.clone()],
            cur,
            None,
            None,
        )
    }

    fn handle_static_asset(&self, filename: &str)
                           -> Result<BinaryResponse, Error> {
            match StaticAsset::get(filename) {
                Some(content) => {
                    let extension: Option<&str> = filename.split('.').last();

                    const DEFAULT_CONTENT_TYPE: &str = "text/plain";
                    let content_type: &str = match extension {
                        Some("css") => "text/css",
                        Some("js") => "text/javascript",
                        Some("html") => "text/html",
                        Some(_) => DEFAULT_CONTENT_TYPE,
                        None => DEFAULT_CONTENT_TYPE,
                    };
                    // there must be a better way
                    let content = content.into_owned();
                    let content_type = content_type_header(content_type);
                    let cur = Cursor::new(content);


                    Ok(tiny_http::Response::new(
                        tiny_http::StatusCode(200),
                        vec![content_type],
                        cur,
                        None,
                        None,
                    ))
                }
                None => {
                    Err(Error::new(
                        ErrorKind::NotFound,
                        format!("Asset \"{}\" not found", filename),
                    ))
                }
            }
        }

    fn save_text(&self, req: &mut tiny_http::Request) -> Result<String, Error> {
        if req.method() != &tiny_http::Method::Post {
            return Err(Error::new(
                ErrorKind::UserError, "Send POST to this path"));
        }

        let mut data = Vec::new();

        req.as_reader()
            .read_to_end(&mut data)
            .map_err(|e| Error::from_io_error(e, "Error receiving the data"))?;

        let mut parser = form_urlencoded::parse(data.as_slice());
        let (k, v) = match parser.next() {
            None => {
                return Err(Error::new(
                    ErrorKind::UserError,
                    "No arguments provided to /text",
                ));
            }
            Some(kv) => kv,
        };
        if k != "text" {
            return Err(Error::new(
                ErrorKind::UserError,
                format!("Invalid parameter \"{}\" with value \"{}\" ", k, v),
            ));
        }

        if let Some((k, v)) = parser.next() {
            return Err(Error::new(
                ErrorKind::UserError,
                format!("Invalid extra parameter \"{}\" with value \"{}\" ",
                        k, v),
            ));
        }

        let mut text = v;

        self.write_text(text.to_mut())
            .map_err(|e| Error::from_io_error(e, "Write error"))?;

        Ok(format!("Saved text: {}", text))
    }

    fn handle_text(&self,  req: &mut tiny_http::Request)
                   -> Result<BinaryResponse, Error> {
        match self.save_text(req) {
            Ok(msg) => Err(Error::new(ErrorKind::Success, msg)),
            Err(err) => {
                Err(Error::new(ErrorKind::ServerError,
                               format!("save text error: {:?}", err)))
            }
        }
    }

    fn save_file_from_request(&self, req: &mut tiny_http::Request)
                              -> Result<(), Error> {
        let mut req = Multipart::from_request(req)
            .map_err(|e| Error::new(ErrorKind::ServerError,
                                    format!("{:?}", e)))?;

        let mut err: Result<(), Error> =
            Err(Error::new(ErrorKind::UserError, "no entries provided"));
        req.foreach_entry(|mut entry| {
            let name = &*entry.headers.name.clone();
            if name == "file" {
                let file = self
                    .create_file(
                        UploadType::File,
                        entry.headers.filename.map(sanitize_filename),
                    )
                    .map_err(|e| Error::from_io_error(e, "create file error"));

                let file = match file {
                    Ok(file) => file,
                    Err(e) => {
                        err = Err(e);
                        return;
                    }
                };

                let result = entry
                    .data
                    .save()
                    .memory_threshold(64 * 1024 * 1024)
                    .write_to(file);

                match result {
                    SaveResult::Full(_) => {}
                    SaveResult::Partial(partial, partial_reason) => {
                        err = Err(Error::new(
                            ErrorKind::Unknown,
                            format!(
                                "data partially saved/received, partial = {}, \
partial_reason = {:?}", partial, partial_reason),
                        ))
                    }
                    SaveResult::Error(error) => {
                        err = Err(Error::new(
                            ErrorKind::ServerError,
                            format!("data save error: {}", error),
                        ));
                    }
                }
            } else {
                err = Err(Error::new(
                    ErrorKind::UserError,
                    format!("invalid entry (expected only \"file\") {}", name),
                ));
            }
        }).map_err(|e| {
            Error::new(
                ErrorKind::ServerError,
                format!("foreach_entry error: {:?}", e),
            )
        })?;

        Ok(())
    }

    fn handle_file_upload(&self, req: &mut tiny_http::Request) ->
        Result<BinaryResponse, Error> {
        match self.save_file_from_request(req) {
            Ok(()) => {
                Err(Error::new(ErrorKind::Success, "File uploaded!"))
            }
            Err(err) => {
                Err(err)
            }
        }
    }

    fn respond(&self, start_t: time::Instant,
               req: tiny_http::Request,
               resp_result: Result<BinaryResponse, Error>) {

        let method = req.method().clone();
        let url = req.url().to_string();

        let resp: BinaryResponse = match resp_result {
            Ok(resp) => resp,
            Err(err) => self.error_response(&err),
        };

        let make_resp_dur = start_t.elapsed();
        let respond_result = req.respond(resp);
        let resp_complete_dur = start_t.elapsed();

        match respond_result {
            Ok(()) => {
                println!(
                    "{:6} [{:8} us, {:8} us] (Ok)  {}",
                    method.as_str(), make_resp_dur.as_micros(), resp_complete_dur.as_micros(), url);

            },
            Err(err) => {
                println!(
                    "{:6} [{:8} us, {:8} us] {} => {:?}",
                    method.as_str(), make_resp_dur.as_micros(), resp_complete_dur.as_micros(), url, err);
            }
        }
    }

    fn handle_request(&self, base_url: &Url, mut req: tiny_http::Request) {
        let start_t = time::Instant::now();

        self.die_if_single_request();

        let url = req.url();

        if url == "/" {
            self.respond(start_t, req, self.handle_home());
            return;
        }

        let url = base_url.join(url).unwrap();
        let mut path_segments = url.path_segments().unwrap();

        match path_segments.next() {
            Some("assets") => {
                if let Some(filename) = path_segments.next() {
                    self.respond(start_t, req, self.handle_static_asset(filename));
                } else {
                    self.respond(start_t, req, Err(
                        Error::new(ErrorKind::NotFound, "/assets is not enumeratable")
                    ));
                }
            }
            Some("text") => {
                let resp = self.handle_text(&mut req);
                self.respond(start_t, req, resp);
            }
            Some("file") => {
                let resp = self.handle_file_upload(&mut req);
                self.respond(start_t, req, resp);
            }
            Some(other) => {
                self.respond(
                    start_t, req,
                    Err(Error::new(
                        ErrorKind::NotFound,
                        format!("There's nothing at /{}", other),
                    )));
            }
            None => {
                unreachable!(
                    "first URL path segment is none but we also didn't handle home, \
                     which should never happen. It's a programmer's error."
                );
            }
        }
    }

    pub fn run(&mut self) {
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
