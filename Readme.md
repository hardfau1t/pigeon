## Yoink

A tool for managing and executing HTTP queries via shell. The main aim of the tool is to easily script queries and run them effortlessly in the shell.

#### Why yet another tool?

You can find list of other tools [here]()
- web/desktop clients  or tools which are embed to IDE are not scriptable.
- [postman](https://www.postman.com/) and [hoppscotch](https://hoppscotch.io/) both has cmdline tools to run queries but they are not that convenient
- Tools which meant for scripting like curl/Httpi etc are not that convenient if you have hundreds of endpoints to manage
- Other tools which are meant for testing are not that convenient to use, Most of the time you need to provide list of endpoints and it will run all of them

## Configuration

`Yoink` will look for `yoink.toml` file in current directory. If you have it in separate place use `-c` flag to indicate location of the file.

Structure of the file looks like below
```toml
# should match the version(major and minor) of the binary
version = "<version string>"
# useful if you are testing in automation provide different names to isolate configs
project = "<project name>"
api_directory = "./services" # Place where services/apis are present
```

### Services

Can also be said as `module`.
`api_directory` contains **services**/**modules**, structure of this directory looks like below
```
svc_a.toml
svc_b/index.toml
svc_b/svc_c.toml
svc_b/svc_d/index.toml
svc_b/svc_d/index.toml
```
Here `svc_a` and `svc_b` are two main services which can have infinitely nested submodules.
Where all of the submodules will inherit parent module environments and you can override
parent environments or create new environments.

Each service or module will have

- At least one environment
- 0 or more endpoints

Where as submodules doesn't require environment they will inherit parent modules environments.
To override parent environment in submodule create Environment with same name as in parent.

The service/module structure looks like below
```toml
alias = "<shortname>"   # Optional short name for module/submodule
description = "<desc>"  # Optional description for given module

# List of environments like dev, prod etc, and hostname/ip of the service
# Required for services/modules but optional for submodules
[environment.<name>]
scheme = "https|http"
host = "<ip address/hostname>"
port = 80           # Its an optional field can range from 0-65535
prefix = "/prefix"  # Optional prefix which will be applied to all endpoints path under this module/submodule
headers = {}        # Optional Map of headers which will be applied to all endpoints, headers in endpoints can override these values
[environment.<name>.store] # Optional map containing key value pair for substitutions
key = "value"

# ... more environments


# endpoints
[endpoint.<name>]
alias = "<short name>" # OPTIONAL, short name for the endpoint
method = "get|post|delete|patch|put|options|head|connect|trace"
path = "<endpoint path>" # url path without ip or scheme or port
# OPTIONAL: headers, list of list of key value pairs
# duplicate headers with same keys are not allowed as per HTTP standards
# if you need to send values for same header then send them as comma separated
headers = {x-abc= "d, e", header-key = "header-value"}
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

[endpoint.<name>.body]
kind = "<content-type>" # this signifies body content-type request headers will be set with given Content-Type
data = "<inline-data>" # body is directly represented in raw string, This conflicts with `path`
path = "<data-file>" # file path containing body, This conflicts with `data`

# You can put submodules in same file as
[submodule.<subm_name>.environment.<name>]
# environment configs
[submodule.<subm_name>.endpoint.<name>]
# endpoint config
```

### Path substitutions

To keep it simple currently we are only supporting substitutions for path part of url.
i.e. `/foo/${bar}` will try to replace bar with `$bar` from environment variable or from [config store](#configuration-store).
If don't want to substitutions then escape `$` with `\`.

**NOTE**: if you are using double quoted strings then you have to double escape it like `"\\\\$abc"`

### Hooks

Hooks takes msgpack serialized data and runs set of operations and writes serialized msgpack data to stdout.
Why msgpack? unlike json or any other formats it can serialize binary data and responses can contain binary data.
As for other formats like json serialized data support might be added later but its not yet supported.

You can pass flags for hooks during runtime. Any flags passed after `--` will be consider to pre-hook.
Second `--` will indicate that any flags after that will be given to post-hook script

Check [example pre-hook](./example-prehook.nu) or [example post-hook](./example-posthook.nu) scripts

Note: for debugging you can write to stderr, which will be printed with log level debug(`-vv`)

Note: whatever is written to stdout is considered as output and gets deserialized

#### Request hook structure
```json
{
  "headers": {
    "a": "b",
    "c": "d",
    "header-key": "header-value",
    "x-abc": "d, e"
  },
  "params": [
    [ "a", "b" ],
    [ "arg", "value" ]
  ],
  "body": null,
  "path": "/get",
  "method": "get",
  "config": {
    "NEST": "dev",
  }
}
```

#### Response Hook structure
```json
{
  "headers": {
    "content-type": "application/octet-stream",
    "date": "Sat, 03 Aug 2024 12:25:36 GMT",
    "server": "gunicorn/19.9.0",
    "content-length": "10",
    "connection": "keep-alive",
    "access-control-allow-origin": "*",
    "access-control-allow-credentials": "true"

  },
  "body": [ 115, 160, 17, 164, 210, 4, 93, 32, 195, 62],
  "status": 200,
  "status_text": "OK",
  "config": {
    "NEST": "dev",
  }
}
```

#### Configuration Store

For every project one config store file is created. For linux it is in `$XDG_CACHE_DIR/Yoink/<project>`.
Its a simple key value pair of strings and these will be used for substitution. Scripts/hooks can set these values
in their return object

Also key values from environment variables will also be used for substitutions.

Priority of these key values is as follows
1. environment variables
2. config store
3. `environment.store` section in services
