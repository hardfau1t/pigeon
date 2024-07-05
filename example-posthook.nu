#!/usr/bin/env -S nu --stdin --no-newline

def main []: binary -> binary {
    let data = from msgpack
    print -e ($data | to nuon)
    $data
    | update body []
    | to msgpack | ^cat
}
