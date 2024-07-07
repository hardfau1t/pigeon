## Pigeon

A tool for managing and executing HTTP queries via shell. The main aim of the tool is to easily script queries and run them effortlessly in the shell.

#### Why yet another tool?

You can find list of other tools [here]()
- web/desktop clients  or tools which are embed to IDE are not scriptable.
- [postman](https://www.postman.com/) and [hoppscotch](https://hoppscotch.io/) both has cmdline tools to run queries but they are not that convenient
- Tools which meant for scripting like curl/Httpi etc are not that convenient if you have hundreds of endpoints to manage
- Other tools which are meant for testing are not that convenient to use, Most of the time you need to provide list of endpoints and it will run all of them

## Configuration

`pigeon` will look for `pigeon.toml` file in current directory. If you have it in separate place use `-c` flag to indicate location of the file.

Structure of the file looks like below
```toml
version = "<version string>"

# List of environments like dev, prod etc, and hostname/ip of the service
[[environment]]
name = "<environment name>"
[[environment.service]]
name = "<service name>"
scheme = "https|http"
host = "<ip address/hostname>"

# ... more services if you have
# ... more environments

# services list

[[service]]
name = "<service name>" # this should match with the environment service name
alias = "<short name>" # OPTIONAL, short service name which can be used while querying
[[service.endpoint]]
name = "<endpoint name>" # identifier for the given endpoint
alias = "<short name>" # OPTIONAL, short name for the endpoint
method = "get|post|delete|patch|put|options|head|connect|trace"
path = "<endpoint path>" # url path without ip or scheme or port
# OPTIONAL: headers, list of list of key value pairs
# NOTE: key should be unique and for 1 key there can be only 1 value.
# exception for this is when key starts with `x-`. Generally this should be avoided as per
# http standards, but this is allowed
headers = [
    ["key", "value"],
    ["x-key", "value-1", "value-2"]
]
# OPTIONAL: query args, here key can be duplicate(unlike headers)
params = [
    ["key", "value"],
    ["key-2", "value-2"],
    ["key-2", "value-3"]
]
# hook or script which are executed on request data
pre_hook.closure = "<inline>" # NOT YET SUPPORTED inline script, commands are directly written in string, conflicts with script
pre_hook.script = "<path>" # path to script file
# hook or script which are executed on response data
post_hook.closure = "<inline>" # NOT YET SUPPORTED inline script, commands are directly written in string, conflicts with script
post_hook.script = "<path>" # path to script file
[service.endpoint.body]
kind = "<content-type>" # this signifies body content-type request headers will be set with given Content-Type
data = "<inline-data>" # body is directly represented in raw string, This conflicts with `path`
path = "<data-file>" # file path containing body, This conflicts with `data`
# list of hooks or scripts which are executed before running query, which can be used to modify headers, body etc
```

### Path substitutions

To keep it simple currently we are only supporting substitutions for path part of url.
i.e. `/foo/${bar}` will try to replace bar with `$bar` from environment variable.
If don't want to substitutions then escape `$` with `\`.
NOTE: if you are using double quoted strings then you have to double escape it.


### Hooks

Hooks takes msgpack serialized data and runs set of operations and writes serialized msgpack data to stdout.
Why msgpack? unlike json or any other formats it can serialize binary data and responses can contain binary data.
As for other formats like json serialized data support might be added later but its not yet supported.

Check [example pre-hook](./example-prehook.nu) or [example post-hook](./example-posthook.nu) scripts

Note: for debugging you can write to stderr, which will be printed with log level debug(`-vv`)

Note: whatever is written to stdout is considered as output and gets deserialized

#### Request hook structure
```
{
  "headers": {
# Note values are represented as list, if duplicate keys are found then all of them are passed as list of values, but if there is a key then it has atleast 1 value
    "content-type": [
      "application/json"
    ],
    "key": [
      "value"
    ]
  },
  "params": [
    [
      "a",
      "b"
    ],
    [
      "arg",
      "value"
    ]
  ],
  "body": [
    123,
    34,
    97,
    34,
    58,
    32,
    34,
    98,
    34,
    44,
    32,
  ],
  "host": "httpbin.org",
  "path": "/post",
  "scheme": "http"
}
```

#### Response Hook structure
```
{
  "headers": {
# Note values are represented as list, if duplicate keys are found then all of them are passed as list of values, but if there is a key then it has atleast 1 value
    "content-type": [
      "application/octet-stream"
    ],
    "content-length": [
      "100"
    ],
    "access-control-allow-origin": [
      "*"
    ]
  },
  "body": [
    38,
    18,
    232,
    245,
    255,
    89,
    255,
    151,
    27,
    200,
    114,
    144,
  ],
  "status": 200,
  "status_text": "OK"
}
```
