## How to Use

`qwicket` will look for `qwicket.toml` file in current directory. This file will have all the necessary details about the collection. If you have it in separate place use `-c` flag to indicate location of the file.

A **Collection** contains bunch of files and directories, each file or a directory is called as **Group**. This is useful to group api's in separate file eventhough all those api's belongs
to same host/server.

A **Group** can be a **Generic** group(directory) which can only contains sub groups(files or directories).
A file can be **HTTP**, **SQL**, **Generic** group(depending on what all are supported) and this contains **Environments**, **Queries** and **Groups**.
Even though a group can contain any other type of groups, it can only contain its own type of environments or queries.

An **Environment** will contain all the necessary information to connect to that host and some generic fields of the query.
**Environment** can inherit its parent environment attributes as long as they are from same group and have the same name.
To select an environment you need to set shell's Environment `export NEST=<environment>`.

Most of the fields of **Query** or **Environment** can be substituted via shell environment variables, store variables or they can
also be specified in **Environments**. If there are any duplicates in above then order of priority is applied as below
1. Shell Environment variables
2. Variables specified in the environments `store` field
3. Variables in the *store*

**store** will contain dynamically generated variables(mostly by hooks) and these variables can be set by using `--set` option of `qwicket` or through hooks.
If possible always prefer environment variables instead of this. Store variables are environment specific if you set a store variable in one environment
and if you change environment then that variables is no longer accessible but it will persistent. When you switch back to the previous environment you can 
access that variable

### Config file

Config file for `qwicket` which contains details about your current project

Structure of the file looks like below
```toml
# should match the version(major and minor) of the binary
version = "<version string>"
# useful if you are testing in automation provide different names to isolate configs
project = "<project name>"
api_directory = "./services" # Place where services/apis are present
```

### Groups

Currently there only 2 types of groups
1. Generic group
2. Http Group

Directories can only be generic group, where as files can be generic as well as specific groups.
Content of file can be as below
```toml
type = "(http|generic)"
# ... Environments, Queries, Groups

[group.<inner_group_name>]
type = "(http|generic)"
```
as per above groups can be nested in a same file or it can be nested via separating using files or directory.
`index.toml` is a special file which can convert its parent directory into non generic group(this way you can add environments or queries to that group)

### Environment

Environment will contain necessary info to connect to server and specific keys or store key value pairs for that given environment.
Structure of the environment is dependent on type of group.

#### Http environment

This can be added in an http group and structure of this looks as below
```toml
[environment.<environment_name>]
scheme = "(http|https)"
host = "<hostname/ip address>"
port = <0-65535> # if this is a default port then it can be skipped
prefix = "<prefix>" # Optional prefix which gets added to HTTP apis,
headers = <map> # optional toml map of headers which are added to all the apis in
                # current group or child groups
store = <map> # Optionnal map containing key value pairs for string substitution
args = <list<list[key, value]>> # list of query args, any duplicate key value pair is kept as it is
```

**NOTE:** joining prefix to query path is done according to [this](https://docs.rs/reqwest/0.12.12/reqwest/struct.Url.html#method.join)

### Query

#### Http Query

Structure of a http query is as below
```toml
[query.<query_name>]
    description: "<description>" # Optional: describes current query
    path: "String" # api path,
    # Method should be in upper case
    # you can give any string as method(useful for custom methods)
    method: "<http method>"
    headers: Map{key = value} # Optional headers for http query
    args: List[List[key, value]] # Optional list of [key, value] pair, where key / value can be duplicate
    # Optional http timeout duration
    # default = 30 secs
    timeout: {secs = int, nanos = int}
    # Optional: Http version
    # default: http11
    version: "(http09|http10|http11|http2|http3)"
    # Optional basic authentication
    # if specified password is optional
    basic_auth: Map{ user_name = "<username>", password = "<password>" }
    # Optional: Bearer authentication token
    bearer_auth: "<Bearer auth token>"
    # Optional: pre request hook
    pre_hook: <Hook>
    # Optional: post response hook
    post_hook: <Hook>
    # Optional http body
    body: <HttpBody>
    # Optional: HTTP form body
    form: Map{key = value}
    # Optional: Multipart body
    multipart: Map{key = part}

```

**NOTE:** joining prefix to query path with environments prefix is done according to [this](https://docs.rs/reqwest/0.12.12/reqwest/struct.Url.html#method.join)

##### Body

Http body can be of specific type(tagged) or raw body. In case of tagged body content type is added automatically
But for raw body, content-type should be mentioned. Body can be inline(mentioned directly) or file(store separately)

body can be created as below
```toml
body."application/json".inline = "<json value>"
# or 
body."application/json".file = "<file path containing json value>"
# or raw file which can contain binary data(this doesn't supports substitution)
body."raw" = {content_type = "<content-type>", file = "<file path containing json value>"}
# or raw text data, (this support substitution)
body."raw_text" = {content_type = "<content-type>", file = "<file path containing json value>" }
```

Here currently supported standard bodies are
- 'application/json'

Body can also be form data, this can be created by
```toml
form = {<key> = <value>, ...}
```

Multipart body can be created by
```toml
[query.<name>.multipart]
<part_name> = <part value>
...
```
Where part name is a string and part value is a map with below structure
```toml
# Optional file name for the part
file_name = "<file_name>"
# Optional headers for given body
headers = Map{key = value}
body.<type> = <Body> # Http body value
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

For developing hooks use `--inspect-request` or `--inspect-response` flag to view the content and create script

Check [example pre-hook](../example-hooks/httpbin/post.nu) or [example post-hook](../example-hooks/httpbin/put.nu) scripts

Note: for debugging you can write to stderr, which will be printed with log level debug(`-vv`)

Note: whatever is written to stdout is considered as output and gets deserialized

#### hook structure

##### HTTP request
```json
{
    "path": "<String>",
    "method": "<String>",
    "headers": "Map{key, value}",
    "args": [["key1", "value1"], ["key2", "value2"],...],
    "timeout": {"secs": "int", "nanos": "int"},
    "version": "String",
    "basic_auth": {"user_name": "<username>", "password": "<password>"},
    "bearer_auth": "String",
    "body": "HttpBody",
    "form": "Http Form body",
    "multipart": "Http multipart body"
}
```
NOTE: In above schema optional fields are same as in Http Query and schema of the each fields is also according to Query

##### HTTP Response
```json
{
    "status_code": "int",
    "version": "String",
    "headers": "Map{String, String}",
    "store": "Map{String, String}",
    "body": "Binary data",
}
```
Where
- `status_code`: http response status code
- `version`: http version
- `headers`: response headers
- `store`: empty environment variables, fill this to update from the hook
- `body`: Raw binary data, You need to decode and parse and re encode it before giving back to the script

#### Configuration Store

For every project one config store file is created. For linux it is in `$XDG_CACHE_DIR/qwicket/<project>`.
Its a simple key value pair of strings and these will be used for substitution. Scripts/hooks can set these values
in their return object

Also key values from environment variables will also be used for substitutions.

Priority of these key values is as follows
1. shell environment variables
2. `environment.store` section in services
3. config store
