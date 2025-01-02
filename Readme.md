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
  "args": {},
  "headers": {
    "Accept": "*/*",
    "Host": "httpbin.org",
    "User-Agent": "qwicket/0.5.0",
    "X-Amzn-Trace-Id": "Root=1-6774ab20-70aa554a05a25d416fce5f55"
  },
  "origin": "152.58.240.73",
  "url": "https://httpbin.org/get"
}
```
You can find more examples under [services](./services) directory. For details check [docs](./docs/readme.md).

#### Usage with Nushell

Combining power of `nushell` with `qwicket` you can generate request / request body on the fly. For example here we will get the response from one query and pipe it to 
another query input
```nu
qwicket httpbin get 
    | from json                             # decode json response
    | update args.abc "changed value"       # update one of the field of input
    | to json                               # encode it back to json
    | { body : {'application/json' : { "inline" : $in }}} # this is the structure of query pass body, you can add anything like headers etc
    | to msgpack                            # qwicket takes input as msgpack so that raw data can also be encoded
    | qwicket httpbin post -s               # pipe it to another query
{
  "args": {},
  "data": "{\n  \"args\": {\n    \"abc\": \"changed value\",\n    \"h\": \"i\"\n  },\n  \"headers\": {\n    \"Accept\": \"*/*\",\n    \"Custom\": \"hab\",\n    \"Host\": \"localhost\",\n    \"User-Agent\": \"qwicket/0.5.0\"\n  },\n  \"origin\": \"172.17.0.1\",\n  \"url\": \"http://localhost/get?abc=def&h=i\"\n}",
  "files": {},
  "form": {},
  "headers": {
    "Accept": "*/*",
    "Content-Length": "252",
    "Content-Type": "application/json",
    "Host": "localhost",
    "User-Agent": "qwicket/0.5.0"
  },
  "json": {
    "args": {
      "abc": "changed value",
      "h": "i"
    },
    "headers": {
      "Accept": "*/*",
      "Custom": "hab",
      "Host": "localhost",
      "User-Agent": "qwicket/0.5.0"
    },
    "origin": "172.17.0.1",
    "url": "http://localhost/get?abc=def&h=i"
  },
  "origin": "172.17.0.1",
  "url": "http://localhost/post"
}
```
NOTE: you can use [json-wrapper](./utils/wrapper.nu) for simplifying this process, where you can directly give nushell output


### Utilities

You can find useful utilities like completion, starship prompt, scripts/aliases under [utils](./utils) folder


## LICENSE

The project is made available under the GPLv3 license. See the LICENSE file for more information.
