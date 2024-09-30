def endpoint-path [context: string] {
    let half_completed = $context !~ '\s+$'
    let flags = ($context | str trim| split row -r '\s+' | skip until {|key| $key =~ '^pigeon$' } | skip)
    if '-j' in $flags {
        []
    } else {
        let ep_path_params = $flags 
            | filter {|flag| not ( $flag| str starts-with '-') } 

        let complete_params = $ep_path_params
            | if $half_completed { drop } else {}

        let half_completed_flag = if $half_completed  { $ep_path_params | last } else { "" }

        let services = (^pigeon -j | from json | get services)

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
}

export extern pigeon [
    --verbose      (-v)
    --config-file  (-c): string                     # configuration file containing queries [default: ./pigeon.toml]
    --no-persistent(-p)                             # don't store changes to config store back to disk
    --output       (-o): string
    --list         (-l)                             # list available options (services/endpoints)
    --json         (-j)                             # output collected services as json output
    --help         (-h)                             # Print help
    --version      (-V)                             # Print version
    ...endpoint           : string@endpoint-path       # path specifier
]
