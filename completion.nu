def endpoint-path [context: string] {
    # for inline exported variables like `key=value pigeon` ast is wierd
    let context = $context | str replace -a -r '\w+=\w+' ''
    let ep_path_params = ast $context -j
        | get block
        | from json
        | get pipelines
        | last
        | get elements.0.expr.expr.Call.arguments.Positional?
        | compact
        | get expr
        | get String

    let half_completed = $context !~ '\s+$'

    let complete_params = $ep_path_params
        | if $half_completed { drop } else {}

    let half_completed_flag = if $half_completed  { $ep_path_params | last } else { "" }

    let services = (^pigeon --list-json | from json | get services)

    $complete_params
        | reduce -f {submodules: $services, endpoints: {} } {|next_path, acc|
            $acc
            | get submodules
            | get $next_path
            | select submodules endpoints
        }
        | get submodules endpoints
        | columns
        | filter {|op| $op | str starts-with $half_completed_flag } # give only words which starts with half completed flags
        | each {|f| $f + " "}       # add extra space at the end
}

export extern pigeon [
    --verbose(-v)                       # verbose logging
    --config-file(-c): string           # <CONFIG_FILE> configuration file containing queries [default: ./pigeon.toml]
    --no-persistent(-p)                 # don't store changes to config store back to disk
    --output(-o): string                # <OUTPUT> output file
    --input(-i): string                 # <INPUT> input file
    --content-type(-t): string          # <CONTENT_TYPE>  content-type of the data. if the services has `kind` then that has higher priority. This should be used with `input` and `kind` is not set in services [default: text/plain]
    --json                              # set content-type to json alias to -t "application/json"
    --list(-l)                          # list available options (services/endpoints)
    --environment(-e): string           # <ENVIRONMENT>    use given environment
    --dry-run(-n)                       # don't run the query just run till pre-hook use with --verbose(-v) to be useful
    --skip-hooks(-s)                    # don't run any hooks
    --skip-prehook                      # don't run pre request hook
    --skip-posthook                     # don't run post responnse hook
    --list-json                         # output collected services as json output
    --help(-h)                          # Print help
    --version(-V)                       # Print version
    ...endpoint : string@endpoint-path  # path specifier
]
