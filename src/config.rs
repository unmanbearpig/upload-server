use std::process;
use crate::error::{Error, ErrorKind};
use crate::srv;
use std::env::Args;
use url::Url;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:2022";

pub struct Config {
    listen_addr: String,
    uploads_dir: String,
}

impl Config {
    pub fn parse_args(args: &mut Args) -> Result<Config, Error> {
        let mut listen_addr: Option<String> = None;
        let mut uploads_dir: Option<String> = None;

        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--help" => {
                    println!(r#"
upload-server:
  HTML server that allows to upload some text or a file that will be saved
    on a filesystem

arguments:
  --help             -- print help and exit
  --listen ADDR      -- listen on address ADDR having format host:port
  --uploads-dir PATH -- save received files and texts into the PATH
"#);
                    process::exit(0);
                },
                "--listen" => {
                    let listen_addr_arg = args.next()
                        .ok_or_else(|| Error::new(
                            ErrorKind::UserError,
                            "Missing argument to --listen"))?;
                    Url::parse(listen_addr_arg.as_ref())
                        .map_err(|e| Error::new(
                            ErrorKind::UserError,
                            format!("invalid url: \"{}\": {:?}",
                                    listen_addr_arg, e)))?;
                    listen_addr = Some(listen_addr_arg);
                },
                "--uploads-dir" => {
                    let uploads_path_arg = args.next()
                        .ok_or_else(|| Error::new(
                            ErrorKind::UserError,
                            "Missing argument to --uploads-dir"))?;
                    uploads_dir = Some(uploads_path_arg);
                },
                other => {
                    return Err(Error::new(ErrorKind::UserError,
                                          format!("Invalid argument \"{}\"", other)));
                }
            }
        }

        let uploads_dir = match uploads_dir {
            Some(dir) => dir,
            None => {
                return Err(
                    Error::new(ErrorKind::UserError,
                               "--uploads-dir argument is required"));
            }
        };

        let listen_addr = listen_addr.unwrap_or_else(|| DEFAULT_LISTEN_ADDR.to_string());

        Ok(Config {
            listen_addr, uploads_dir
        })
    }

    pub fn make_server(&self) -> srv::Srv {
        let http = match tiny_http::Server::http::<&str>(self.listen_addr.as_ref()) {
            Ok(http) => http,
            Err(e) => panic!("http start error: {:?}", e),
        };

        let base_url = Url::parse(format!("http://{}", self.listen_addr).as_ref()).unwrap();

        srv::Srv::new(http, base_url, self.uploads_dir.as_ref())
    }
}
