# Upload-server

Allows to upload something to your computer.

Uses minimal dependencies, no async stuff. Compiles slowly anyway.

Code quality is poor.

# Build

```
cargo build --release
```

# Running

```
upload-server --uploads-dir ~/tmp
```

Will store files and sent text into files named
```
{date}--{time}--{filename}--payload
```

# Arguments

  --help             -- Print help and exit

  --listen ADDR      -- Listen on address ADDR having format host:port
                        default is {default_listen_addr}

  --uploads-dir PATH -- Save received files and texts into the PATH
                        default is {default_uploads_dir}

  --name NAME        -- Say that name on the home page
                        default is {default_name}

  --save-meta        -- Also create metadata files
