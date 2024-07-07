#!/usr/bin/env -S nu --stdin --no-newline

def main [
    --verbose(-v), # print input and output to stderr
]: binary -> binary {
    let data = from msgpack
    if $verbose {
        print -e ($data | to nuon)
    }
    $data
    | update headers {|row|  $row.headers | insert foo [bar] }
    | update params {|msg| $msg.params | append  [["arg" , "value"]]}
    | to msgpack | ^cat
}
