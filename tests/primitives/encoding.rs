// Copyright (c) ZeroC, Inc.

mod slice1 {

    use crate::test_helpers::*;
    use slice::diagnostics::{Error, ErrorKind};
    use slice::grammar::Encoding;
    use test_case::test_case;

    /// Verifies that if Slice1 is used with unsupported types (int8, uint16, uint32, varint32,
    /// varuint32, uint64, varint62, and varuint62) that the compiler will produce the relevant not
    /// supported errors.
    #[test_case("int8"; "int8")]
    #[test_case("uint16"; "uint16")]
    #[test_case("uint32"; "uint32")]
    #[test_case("varint32"; "varint32")]
    #[test_case("varuint32"; "varuint32")]
    #[test_case("uint64"; "uint64")]
    #[test_case("varint62"; "varint62")]
    #[test_case("varuint62"; "varuint62")]
    fn unsupported_types_fail(value: &str) {
        // Test setup
        let slice = &format!(
            "
                encoding = Slice1
                module Test

                compact struct S {{
                    v: {value}
                }}
            "
        );

        // Act
        let diagnostics = parse_for_diagnostics(slice);

        // Assert
        let expected = Error::new(ErrorKind::UnsupportedType {
            kind: value.to_owned(),
            encoding: Encoding::Slice1,
        })
        .add_note("file encoding was set to Slice1 here:", None);

        check_diagnostics(diagnostics, [expected]);
    }

    /// Verifies that valid Slice1 types (bool, uint8, int16, int32, int64, float32, float64,
    /// string, and  AnyClass) will not produce any compiler errors.
    #[test_case("bool"; "bool")]
    #[test_case("uint8"; "uint8")]
    #[test_case("int16"; "int16")]
    #[test_case("int32"; "int32")]
    #[test_case("int64"; "int64")]
    #[test_case("float32"; "float32")]
    #[test_case("float64"; "float64")]
    #[test_case("string"; "string")]
    #[test_case("ServiceAddress"; "ServiceAddress")]
    #[test_case("AnyClass"; "AnyClass")]
    fn supported_types_succeed(value: &str) {
        // Arrange
        let slice = &format!(
            "
            encoding = Slice1
            module Test

            compact struct S {{
                v: {value}
            }}
        "
        );

        // Act/Assert
        assert_parses(slice);
    }
}

mod slice2 {

    use crate::test_helpers::*;
    use slice::diagnostics::{Error, ErrorKind};
    use slice::grammar::Encoding;
    use test_case::test_case;

    /// Verifies that if Slice2 is used with unsupported types (AnyClass) that the compiler will
    /// produce the relevant not supported errors.
    #[test]
    fn unsupported_types_fail() {
        // Arrange
        let slice = "
            module Test

            compact struct S {
                v: AnyClass
            }
        ";

        // Act
        let diagnostics = parse_for_diagnostics(slice);

        // Assert
        let expected = Error::new(ErrorKind::UnsupportedType {
            kind: "AnyClass".to_owned(),
            encoding: Encoding::Slice2,
        })
        .add_note("file is using the Slice2 encoding by default", None)
        .add_note(
            "to use a different encoding, specify it at the top of the slice file\nex: 'encoding = Slice1'",
            None,
        );

        check_diagnostics(diagnostics, [expected]);
    }

    /// Verifies that valid Slice2 types (bool, int8, uint8, int16, uint16, int32, uint32,
    /// varint32, varuint32, int64, uint64, varint62, varuint62, float32, float64, and string) will
    /// not produce any compiler errors.
    #[test_case("bool"; "bool")]
    #[test_case("int8"; "int8")]
    #[test_case("uint8"; "uint8")]
    #[test_case("int16"; "int16")]
    #[test_case("uint16"; "uint16")]
    #[test_case("int32"; "int32")]
    #[test_case("uint32"; "uint32")]
    #[test_case("varint32"; "varint32")]
    #[test_case("varuint32"; "varuint32")]
    #[test_case("int64"; "int64")]
    #[test_case("uint64"; "uint64")]
    #[test_case("varint62"; "varint62")]
    #[test_case("varuint62"; "varuint62")]
    #[test_case("float32"; "float32")]
    #[test_case("float64"; "float64")]
    #[test_case("string"; "string")]
    fn supported_types_succeed(value: &str) {
        // Arrange
        let slice = format!(
            "
            module Test

            compact struct S {{
                v: {value}
            }}"
        );

        // Act/Assert
        assert_parses(slice);
    }

    #[test_case("uint8?"; "optional uint8")]
    #[test_case("uint16?"; "optional uint16")]
    #[test_case("uint32?"; "optional uint32")]
    #[test_case("uint64?"; "optional uint64")]
    #[test_case("int8?"; "optional int8")]
    #[test_case("int16?"; "optional int16")]
    #[test_case("int32?"; "optional int32")]
    #[test_case("int64?"; "optional int64")]
    #[test_case("varint32?"; "optional varint32")]
    #[test_case("varuint32?"; "optional varuint32")]
    #[test_case("varint62?"; "optional varint62")]
    #[test_case("varuint62?"; "optional varuint62")]
    #[test_case("string?"; "optional string")]
    #[test_case("bool?"; "optional bool")]
    #[test_case("sequence<int32>?"; "optional sequence")]
    #[test_case("float32?"; "optional float32")]
    #[test_case("float64?"; "optional float64")]
    fn supported_optional_types_succeed(value: &str) {
        // Arrange
        let slice = format!(
            "
                module Test

                struct MyStruct {{
                    myVar: {value}
                }}
            "
        );

        // Act/Assert
        assert_parses(slice);
    }
}
