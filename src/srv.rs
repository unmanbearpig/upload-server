use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path;

use std::io::Cursor;
use url::Url;

use std::process;
use std::thread;
use std::time;
use std::borrow::Cow;

use multipart::server::{Multipart, SaveResult};

use crate::sanitize_filename::sanitize_filename;

#[derive(RustEmbed)]
#[folder = "assets"]
struct StaticAsset;

use crate::error::{Error, ErrorKind};

fn content_type_header(value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(&b"Content-Type"[..], value).unwrap()
}

pub struct Srv<'a, 'b> {
    http: tiny_http::Server,
    base_url: Url,
    html_content_type: tiny_http::Header,
    die_after_single_request: bool,
    output_path: &'a str,
    send_to_name: &'b str,

    /// Also create metadata files
    save_metadata: bool,
}

#[derive(Debug, Clone, Copy)]
enum UploadType {
    Text,
    File,
}

/// Replaces all occurances of `search_for` with `replace_with` and returns
/// new Vec leaving the original unchanged.
/// Assumes all arguments are valid UTF-8.
/// Probably should do the conversion outside of it.
/// It's a shitty implementation since it allocates a new string and copies it
/// back to the original string (or Vec I should say), which is avoidable,
/// but I'm lazy at the moment.
unsafe fn search_and_replace(content: Vec<u8>,
                             search_for: &[u8],
                             replace_with: &[u8]) -> Vec<u8> {
    let search_for = std::str::from_utf8_unchecked(search_for);
    let replace_with = std::str::from_utf8_unchecked(replace_with);
    let content: String = String::from_utf8_unchecked(content);
    let newstr = content.replace(search_for, replace_with);
    newstr.into_bytes()
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

fn filename_to_content_type<T: AsRef<str>>(filename: T) -> &'static str {
    let filename = filename.as_ref();
    let extension: Option<&str> = filename.split('.').last();

    const DEFAULT_CONTENT_TYPE: &str = "text/plain";
    match extension {
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("html") => "text/html",
        Some(_) => DEFAULT_CONTENT_TYPE,
        None => DEFAULT_CONTENT_TYPE,
    }
}

/// A type of file that we store on the filesystem
#[derive(Clone, Copy, Eq, PartialEq)]
enum FileType {
    Payload,
    Metadata,
}

impl fmt::Display for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileType::Payload  => write!(f, "payload"),
            FileType::Metadata => write!(f, "metadata"),
        }
    }

}

fn mangle_filename<T: AsRef<str>>(
    now: chrono::DateTime<chrono::Local>,
    typ: UploadType, file_type: FileType,
    name: Option<T>) -> String
{
    let date_str = now.format("%F--%T.%f").to_string();

    match name {
        Some(name) => format!(
            "{}--{}--{}--{}",
            date_str, name.as_ref(), typ.as_file_suffix(), file_type),
        None => format!("{}--{}--{}",
                        date_str, typ.as_file_suffix(), file_type),
    }
}

impl<'a, 'b> Srv<'a, 'b> {
    pub fn new(http: tiny_http::Server,
               base_url: Url,
               output_path: &'a str,
               send_to_name: &'b str,
               save_metadata: bool)
               -> Self {
        Srv {
            http,
            base_url,
            html_content_type: content_type_header("text/html"),
            die_after_single_request: false,
            output_path,
            send_to_name,
            save_metadata,
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


    fn create_file<T: AsRef<str>>(
        &self, now: chrono::DateTime<chrono::Local>, typ: UploadType,
        file_type: FileType, name: Option<T>) -> io::Result<fs::File>
    {
        let filename = mangle_filename(now, typ, file_type, name);
        let path = path::Path::new(self.output_path).join(filename);
        fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create_new(true)
            .open(path)
    }

    fn write_text(
        &self, now: chrono::DateTime<chrono::Local>,
        text: &str) -> io::Result<()>
    {
        let mut file = self.create_file::<&str>(
            now, UploadType::Text, FileType::Payload, None)?;
        let bytes: &[u8] = text.as_bytes();
        file.write_all(bytes)?;

        Ok(())
    }

    // TODO cache the content with replaced name
    fn handle_home(&self) ->
        Result<tiny_http::Response<Cursor<Cow<[u8]>>>, Error>
    {
        const HOME_FILENAME: &str = "home.html";
        let content = StaticAsset::get("home.html")
            .ok_or_else(|| Error::new(
                ErrorKind::ServerError,
                format!("Home: {} not found", HOME_FILENAME)))?;

        let content = content.into_owned();

        let content = unsafe {
            search_and_replace(
                content, b"#{name}", self.send_to_name.as_bytes())
        };
        let content = Cow::from(content);

        let content_type = content_type_header("text/html");
        let cur = Cursor::new(content);

        Ok(tiny_http::Response::new(
            tiny_http::StatusCode(200),
            vec![content_type],
            cur,
            None,
            None,
        ))}

    fn error_response(&self, err: &Error) -> tiny_http::Response<Cursor<Cow<[u8]>>> {
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
        let cur = Cursor::new(Cow::from(data));
        tiny_http::Response::new(
            tiny_http::StatusCode(err.as_http_code()),
            vec![self.html_content_type.clone()],
            cur,
            None,
            None,
        )
    }

    fn handle_static_asset(&self, filename: &str) ->
        Result<tiny_http::Response<Cursor<Cow<[u8]>>>, Error>
    {
        match StaticAsset::get(filename) {
            Some(content) => {
                let content_type = filename_to_content_type(filename);

                // there must be a better way
                let content = Cow::from(content);
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

    fn write_metadata<S: AsRef<str>>(
        &self, now: chrono::DateTime<chrono::Local>,
        upload_type: UploadType, name: Option<S>,
        req: &tiny_http::Request) -> Result<(), Error>
    {
        if !self.save_metadata {
            return Ok(())
        }

        let mut meta_file = self.create_file(
            now, upload_type, FileType::Metadata, name)
            .map_err(|e| Error::from_io_error(e, "create metadata file error"))?;

        meta_file.write_fmt(
            format_args!("{} {}\n\n", req.method(), req.http_version()))
            .map_err(|e| Error::from_io_error(e, "write metadata"))?;

        for h in req.headers().iter() {
            meta_file.write_fmt(format_args!("{}: {}\n", h.field, h.value))
                .map_err(|e| Error::from_io_error(e, "write metadata"))?;
        }
        Ok(())
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

        let now: chrono::DateTime<chrono::Local> =
            chrono::offset::Local::now();
        self.write_metadata::<&str>(now, UploadType::Text, None, req)?;

        self.write_text(now, text.to_mut())
            .map_err(|e| Error::from_io_error(e, "Write error"))?;

        Ok(format!("Saved text: {}", text))
    }

    fn handle_text(&self,  req: &mut tiny_http::Request)
                   -> Result<tiny_http::Response<Cursor<Cow<[u8]>>>, Error> {
        match self.save_text(req) {
            Ok(msg) => Err(Error::new(ErrorKind::Success, msg)),
            Err(err) => {
                Err(Error::new(ErrorKind::ServerError,
                               format!("save text error: {:?}", err)))
            }
        }
    }

    /// Saves the uploaded file
    fn save_file_from_request(&self, req: &mut tiny_http::Request)
                              -> Result<(), Error> {
        let now: chrono::DateTime<chrono::Local> =
            chrono::offset::Local::now();
        self.write_metadata(now, UploadType::File, Some("upload"), req)?;

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
                        now,
                        UploadType::File,
                        FileType::Payload,
                        entry.headers.filename.map(sanitize_filename),
                    ).map_err(|e| Error::from_io_error(e, "create file error"));

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
        Result<tiny_http::Response<Cursor<Cow<[u8]>>>, Error> {
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
               resp_result: Result<tiny_http::Response<Cursor<Cow<[u8]>>>, Error>) {

        let method = req.method().clone();
        let url = req.url().to_string();

        let resp: tiny_http::Response<Cursor<Cow<[u8]>>> = match resp_result {
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
