#!/usr/bin/env -S nu --stdin --no-newline

def main []: binary -> binary {
    from msgpack
    | update headers.abc [huh] 
    | update headers {|row|  $row.headers | insert foo [bar] }
    | update params {|msg| $msg.params | append  [["arg" , "value"]]}
    | to msgpack | ^cat
}
