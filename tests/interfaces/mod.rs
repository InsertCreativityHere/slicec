// Copyright (c) ZeroC, Inc. All rights reserved.

mod operations;

use crate::helpers::parsing_helpers::*;
use slice::grammar::*;

#[test]
fn can_have_no_operations() {
    let slice = "
        module Test;
        interface I {}
    ";

    let ast = parse_for_ast(slice);
    let interface_ptr = ast.find_typed_type::<Interface>("Test::I").unwrap();
    let interface_def = interface_ptr.borrow();
    assert_eq!(interface_def.identifier(), "I");
    assert_eq!(interface_def.operations().len(), 0);
}

#[test]
fn can_have_one_operation() {
    let slice = "
        module Test;
        interface I
        {
            op1();
        }
    ";

    let ast = parse_for_ast(slice);
    let interface_ptr = ast.find_typed_type::<Interface>("Test::I").unwrap();
    let interface_def = interface_ptr.borrow();

    assert_eq!(interface_def.operations().len(), 1);
}

#[test]
fn can_have_multiple_operation() {
    let slice = "
        module Test;
        interface I
        {
            op1();
            op2();
            op3();
        }
    ";

    let ast = parse_for_ast(slice);
    let interface_ptr = ast.find_typed_type::<Interface>("Test::I").unwrap();
    let interface_def = interface_ptr.borrow();

    assert_eq!(interface_def.operations().len(), 3);
}
