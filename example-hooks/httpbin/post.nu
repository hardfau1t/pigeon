#!/usr/bin/env -S nu --stdin --no-newline

def main [] {
    from msgpack 
    | update body {
        from json 
        | update 'a' 'c' 
        | to json -r
    }
    | to msgpack
    | ^cat
}
