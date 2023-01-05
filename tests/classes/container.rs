// Copyright (c) ZeroC, Inc. All rights reserved.

use crate::assert_errors;
use crate::helpers::parsing_helpers::{parse_for_ast, parse_for_diagnostics};
use slice::diagnostics::{Error, ErrorKind};
use slice::grammar::*;
use test_case::test_case;

/// Verifies that classes can contain data members.
#[test]
fn can_contain_data_members() {
    // Arrange
    let slice = "
        encoding = 1;
        module Test;
        class C
        {
            i: int32,
            s: string,
            b: bool,
        }
    ";

    // Act
    let ast = parse_for_ast(slice);

    // Assert
    let data_members = ast.find_element::<Class>("Test::C").unwrap().members();

    assert_eq!(data_members.len(), 3);
    assert!(matches!(data_members[0].identifier(), "i"));
    assert!(matches!(data_members[1].identifier(), "s"));
    assert!(matches!(data_members[2].identifier(), "b"));
    assert!(matches!(
        data_members[0].data_type.concrete_type(),
        Types::Primitive(Primitive::Int32),
    ));
    assert!(matches!(
        data_members[1].data_type.concrete_type(),
        Types::Primitive(Primitive::String),
    ));
    assert!(matches!(
        data_members[2].data_type.concrete_type(),
        Types::Primitive(Primitive::Bool),
    ));
}

#[test_case(
    "
        class C
        {
            c: C,
        }
    "; "single class circular reference"
)]
#[test_case(
    "
        class C1
        {
            c2: C2,
        }
        class C2
        {
            c1: C1,
        }
    "; "multi class circular reference"
)]
fn cycles_are_allowed(cycle_string: &str) {
    let slice = format!(
        "
            encoding = 1;
            module Test;
            {cycle_string}
        "
    );

    let diagnostic_reporter = parse_for_diagnostics(slice);

    assert_errors!(diagnostic_reporter);
}

/// Verifies that classes can be empty
#[test]
fn can_be_empty() {
    // Arrange
    let slice = "
        encoding = 1;
        module Test;
        class C
        {
        }
    ";

    // Act
    let ast = parse_for_ast(slice);

    // Assert
    let data_members = ast.find_element::<Class>("Test::C").unwrap().members();
    assert_eq!(data_members.len(), 0);
}

#[test]
fn cannot_redefine_data_members() {
    // Arrange
    let slice = "
        encoding = 1;
        module Test;
        class C
        {
            a: int32,
            a: string,
        }
    ";

    // Act
    let diagnostic_reporter = parse_for_diagnostics(slice);

    // Assert
    let expected = [Error::new(ErrorKind::Redefinition {
        identifier: "a".to_string(),
    })
    .add_note("'a' was previously defined here", None)];
    assert_errors!(diagnostic_reporter, &expected);
}
