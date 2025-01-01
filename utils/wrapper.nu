def --wrapped qj [...rest: string@endpoint-path] {
    let input_data = $in
    let op = if ($input_data | is-empty) {
         (^qwicket ...$rest)
    } else {
         $input_data 
         | to json -r 
         | {body: {"application/json" : {inline: $in }}}
         | to msgpack
         | (^qwicket ...$rest)
    }
    try {
        $op | from json
    } catch {
        $op
    }
}
