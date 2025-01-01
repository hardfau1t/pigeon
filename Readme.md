## Qwicket

A tool for managing and executing queries via shell. The main aim of the tool is to easily script queries and run them effortlessly in the shell.

### Supported features
---
- [x] HTTP
    - http/https
    - basic/bearer auth
    - form requests
    - multipart requests
- [ ] SQL

#### Why yet another tool?

You can find list of other tools [here]()
- web/desktop clients  or tools which are embed to IDE are not scriptable.
- [postman](https://www.postman.com/) and [hoppscotch](https://hoppscotch.io/) both has cmdline tools to run queries but they are not that convenient
- Tools which meant for scripting like curl/Httpi etc are not that convenient if you have hundreds of endpoints to manage
- Other tools which are meant for testing are not that convenient to use, Most of the time you need to provide list of endpoints and it will run all of them

## Installation

You need to have rust-toolchain setup in your machine. (rustup.rs)
1. Using crates.io:
    `cargo install qwicket`
2. Manual build:
    - `git clone https://github.com/hardfau1t/qwicket.git`
    - `cd qwicket`
    - `cargo install --path .`

## Running

Create a main config file `qwicket.toml`.
```toml
version = "0.5.0"
project = "myproject"
api_directory = "./services"
```
Then in `./services` directory create a file. ex `httpbin.toml`
```toml
type = "http"

[environment.prod]
scheme = "https"
host = "httpbin.org"

[query.foo]
description = "Calls `/get` api of httpbin.org"
method = "get"
path = "/get"
```
Set a environment variable `export NEST=prod`.
Then run `qwicket httpbin foo`. This will print
```json
{
  "args": {
    "abc": "def",
    "h": "i"
  },
  "headers": {
    "Accept": "*/*",
    "Custom": "hab",
    "Host": "httpbin.org",
    "User-Agent": "qwicket/0.5.0",
    "X-Amzn-Trace-Id": "Root=1-6774a4c3-661269933d819f9331b7d66c"
  },
  "origin": "152.58.240.73",
  "url": "https://httpbin.org/get?abc=def&h=i"
}
```
You can find more examples under [services](./services) directory. For details check [docs](./docs/readme.md).

## LICENSE

The project is made available under the GPLv3 license. See the LICENSE file for more information.
