#!/usr/bin/env -S nu --stdin --no-newline

def main [] {
    from msgpack
    | update body {
        decode
        | from json
        | update args { insert arg1 value1}
        | to json
        | encode utf-8
    }
    | update store {
        insert key val
    }
    | to msgpack
    |^cat
}
