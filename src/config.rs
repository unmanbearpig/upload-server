use std::process;
use crate::srv;
use std::env::Args;
use url::Url;
use std::fs;
use crate::error::WhateverError;

const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:2022";
const DEFAULT_UPLOADS_DIR: &str = "/var/upload-server/uploads";
const DEFAULT_SEND_TO_NAME: &str = "Anonymousse";

pub struct Config {
    pub listen_addr:   String,
    pub uploads_dir:   String,
    pub send_to_name:  String,
    pub save_metadata: bool,
}

type Error = Box<dyn std::error::Error>;

/// Return Ok(path) if directory is suitable for using for uploads, Err otherwise
fn check_upload_dir<T: AsRef<str>>(path: T) -> Result<T, Error> {
    let metadata = fs::metadata(path.as_ref())?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a directory", path.as_ref()).into());
    }
    if metadata.permissions().readonly() {
        return Err(format!("Direcotry {} is not writable", path.as_ref()).into())
    }

    Ok(path)
}


fn print_help() {
    println!(r#"
upload-server:
  HTML server that allows to upload some text or a file that will be saved
    on a filesystem

arguments:
  --help             -- Print help and exit
  --listen ADDR      -- Listen on address ADDR having format host:port
                        default is {default_listen_addr}
  --uploads-dir PATH -- Save received files and texts into the PATH
                        default is {default_uploads_dir}
  --name NAME        -- Say that name on the home page
                        default is {default_name}
  --save-meta        -- Also create metadata files
"#, default_listen_addr = DEFAULT_LISTEN_ADDR,
             default_uploads_dir = DEFAULT_UPLOADS_DIR,
             default_name = DEFAULT_SEND_TO_NAME,
    );
}


impl Config {
    pub fn parse_args(args: &mut Args) -> Result<Config, Error> {
        let mut listen_addr: Option<String> = None;
        let mut uploads_dir: Option<String> = None;
        let mut send_to_name: String = DEFAULT_SEND_TO_NAME.to_string();
        let mut save_metadata: bool = false;

        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--help" => {
                    print_help();
                    process::exit(0);
                },
                "--listen" => {
                    let listen_addr_arg = args.next()
                        .ok_or_else(|| WhateverError::from(
                            "Missing argument to --listen"))?;
                    listen_addr = Some(listen_addr_arg);
                },
                "--uploads-dir" => {
                    let uploads_path_arg = args.next()
                        .ok_or_else(|| WhateverError::from(
                            "Missing argument to --uploads-dir"))?;
                    uploads_dir = Some(uploads_path_arg);
                },
                "--name" => {
                    let name = args.next()
                        .ok_or_else(|| WhateverError::from(
                            "Missing argument to --name"))?;
                    send_to_name = name;
                },
                "--save-meta" => save_metadata = true,
                other => {
                    return Err(
                        format!("Invalid argument \"{}\"", other).into());
                }
            }
        }

        let uploads_dir = match uploads_dir {
            Some(dir) => {
                check_upload_dir(dir)?
            },
            None => {
                check_upload_dir(DEFAULT_UPLOADS_DIR.to_string())
                    .map_err(|e| format!(
                        "Cannot use default directory {}, use --uploads-dir \
                         or fix the error: {}", DEFAULT_UPLOADS_DIR, e))?
            }
        };

        let listen_addr = listen_addr
            .unwrap_or_else(|| DEFAULT_LISTEN_ADDR.to_string());

        Ok(Config { listen_addr, uploads_dir, send_to_name, save_metadata })
    }

    pub fn make_server(&self) -> srv::Srv {
        let srv = tiny_http::Server::http::<&str>(self.listen_addr.as_ref());
        let http = match srv {
            Ok(http) => http,
            Err(e) => panic!("http start error: {:?}", e),
        };

        let base_url =
            Url::parse(format!("http://{}", self.listen_addr).as_ref())
            .unwrap();

        srv::Srv::new(
            http,
            base_url,
            self.uploads_dir.as_ref(),
            self.send_to_name.as_ref(),
            self.save_metadata)
    }
}
