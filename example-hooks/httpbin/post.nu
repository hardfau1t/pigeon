#!/usr/bin/env -S nu --stdin --no-newline

def main [] {
    from msgpack 
    | update body {
        from json 
        | update 'a' 'c' 
        | to json -r
    }
    | to msgpack
    | ^cat # problem with nu that output of a script is given to stdout as it is, binary should have been converted to raw data
}
