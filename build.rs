// Copyright (c) ZeroC, Inc. All rights reserved.

extern crate lalrpop;

fn main() {
    // Recursively finds any files ending with `.lalrpop` in the `src` directory and generates parsers from them.
    lalrpop::process_root().unwrap();
}
