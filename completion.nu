export def endpoint-path [context: string] {
    # for inline exported variables like `key=value pigeon` ast is wierd
    let context = $context | str replace -a -r '\w+=\w+' ''
    let ep_path_params = ast $context -j
        | get block
        | from json
        | get pipelines
        | last
        | get elements
        | last
        | get expr.expr.Call.arguments.Positional?
        | compact
        | get expr
        | get String

    let half_completed = $context !~ '\s+$'

    let complete_params = $ep_path_params
        | if $half_completed { drop } else {}

    let half_completed_flag = if $half_completed  { $ep_path_params | last } else { "" }

    let query_map = ^pigeon --list-json ...$complete_params
    | from json 
    | get group
    let groups = $query_map.sub_groups | columns
    let queries = $query_map.queries.query? | default {} | columns
    $groups ++ $queries
    | filter {|op| $op | str starts-with $half_completed_flag } # give only words which starts with half completed flags

}

export extern pigeon [
  --verbose(-v),
  --config-file(-c): path           # configuration file containing queries [default: ./pigeon.toml]
  --no-persistent(-p)               # don't store changes to config store back to disk
  --output(-o): path
  --input(-i)
  --list(-l)                        # list available options (services/endpoints)
  --environment(-e): string         # use given environment
  --dry-run(-n)                     # don't run the query just run till pre-hook use with --verbose(-v) to be useful
  --skip-hooks(-s)                  # don't run any hooks
      --skip-prehook                # don't run pre request hook
      --skip-posthook               # don't run post responnse hook
      --inspect-request             # stop before pre hook and write pre hook data to stdout. Useful for developing pre-hook
      --inspect-response            # stop before post hook and write post hook data to stdout. Useful for developing post-hook
      --list-json                   # output collected services as json output
  --version(-V)                     # Print version
    ...endpoint : string@endpoint-path  # path specifier
]
