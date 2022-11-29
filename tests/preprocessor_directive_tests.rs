// Copyright (c) ZeroC, Inc. All rights reserved.

mod command_line {

    use slice::command_line::{DiagnosticFormat, SliceOptions};
    use slice::compile_from_strings;
    use slice::grammar::*;

    fn default_options() -> SliceOptions {
        SliceOptions {
            sources: vec![],
            references: vec![],
            warn_as_error: true,
            disable_color: false,
            diagnostic_format: DiagnosticFormat::Human,
            dry_run: false,
            output_dir: None,
            definitions: vec![],
        }
    }

    #[test]
    fn command_line_defined_symbols() {
        // Arrange
        let slice = "
        module Test;

        # if Foo
        interface I
        {
            op();
        }
        # endif
        ";

        let mut default_options = default_options();
        default_options.definitions = vec!["Foo".to_string()];

        // Act
        let compilation_data = compile_from_strings(&[slice], Some(default_options)).unwrap();

        // Assert
        assert!(compilation_data.ast.find_element::<Operation>("Test::I::op").is_ok());
    }
}
