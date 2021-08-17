// Copyright (c) ZeroC, Inc. All rights reserved.

// TODO split into SliceFile and Util files! No need to keep together!

use crate::cs_util::*;
use crate::decoding::*;
use crate::encoding::*;
use slice::ast::{Ast, Node};
use slice::grammar::*;
use slice::ref_from_node;
use slice::util::{SliceFile, TypeContext};
use slice::visitor::Visitor;
use slice::writer::Writer;
use std::io;

macro_rules! write_fmt {
    ($writer:expr, $fmt:literal, $($arg:tt)*) => {{
        let content = format!($fmt, $($arg)*);
        $writer.write(&content);
    }};
}

pub struct CsWriter {
    output: Writer,
}

impl CsWriter {
    pub fn new(path: &str) -> io::Result<Self> {
        let output = Writer::new(&(path.to_owned() + ".cs"))?;
        Ok(CsWriter { output })
    }

    pub fn close(self) {
        self.output.close();
    }

    /// Helper method that checks if a named symbol has a comment written on it, and if so, formats
    /// it as a C# style doc comment and writes it to the underlying output.
    fn write_comment(&mut self, named_symbol: &dyn NamedSymbol) {
        // If the symbol has a doc comment attached to it, write it's fields to the output.
        if let Some(comment) = &named_symbol.comment() {
            // Write the comment's summary message if it has one.
            if !comment.message.is_empty() {
                self.write_comment_field("summary", &comment.message, "");
            }

            // Write each of the comment's parameter fields.
            for param in &comment.params {
                let (identifier, description) = param;
                let attribute = format!(r#" name="{}""#, &identifier);
                self.write_comment_field("param", &description, &attribute);
            }

            // Write the comment's returns message if it has one.
            if let Some(returns) = &comment.returns {
                self.write_comment_field("returns", &returns, "");
            }

            // Write each of the comment's exception fields.
            for exception in &comment.throws {
                let (exception, description) = exception;
                let attribute = format!(r#" cref="{}""#, &exception);
                self.write_comment_field("exceptions", &description, &attribute);
            }
        }
    }

    fn write_comment_field(&mut self, field_name: &str, content: &str, attribute: &str) {
        let mut field_string = format!("/// <{}{}>", field_name, attribute);
        if !content.is_empty() {
            // Iterate through each line of the field's content, and at the end of each line, append a
            // newline followed by 3 forward slashes to continue the comment.
            for line in content.lines() {
                field_string += line;
                field_string += "\n/// ";
            }
            // Remove the trailing newline and slashes by truncating off the last 5 characters.
            field_string.truncate(field_string.len() - 5);
        }
        // Append a closing tag, and write the field.
        field_string = field_string + "</" + field_name + ">\n";
        self.output.write(&field_string);
    }
}

impl Visitor for CsWriter {
    fn visit_file_start(&mut self, slice_file: &SliceFile, _: &Ast) {
        write_fmt!(
            self.output,
            "\
// Copyright (c) ZeroC, Inc. All rights reserved.

// <auto-generated/>
// slicec-cs version: '{version}'
// Generated from file: '{file}.ice'

#nullable enable

#pragma warning disable 1591 // Missing XML Comment",
            version = env!("CARGO_PKG_VERSION"),
            file = slice_file.filename
        );
    }

    fn visit_file_end(&mut self, _: &SliceFile, _: &Ast) {
        self.output.write("\n")
    }

    fn visit_module_start(&mut self, module_def: &Module, _: usize, _: &Ast) {
        self.write_comment(module_def);
        let content = format!("\nnamespace {}\n{{", module_def.identifier());
        self.output.write(&content);
        self.output.indent_by(4);
    }

    fn visit_module_end(&mut self, _: &Module, _: usize, _: &Ast) {
        self.output.clear_line_separator();
        self.output.indent_by(-4);
        self.output.write("\n}");
        self.output.write_line_separator();
    }

    fn visit_struct_start(&mut self, struct_def: &Struct, _: usize, _: &Ast) {
        self.write_comment(struct_def);

        //TODO: this stuff from slice2cs
        // emitDeprecate(p, false, _out);
        // emitCommonAttributes();
        // emitCustomAttributes(p);

        let struct_modifier = if struct_def.has_attribute("cs:readonly") {
            "public readonly"
        } else {
            "public"
        };

        let content = format!(
            "\n{} partial struct {name} : global::System.IEquatable<{name}>\n{{",
            struct_modifier,
            name = struct_def.identifier()
        );
        self.output.write(&content);
        self.output.indent_by(4);
    }

    fn visit_struct_end(&mut self, struct_def: &Struct, _: usize, ast: &Ast) {
        write_equality_operators(&mut self.output, struct_def.identifier());

        self.output.write_line_separator();

        let mut constructor_args = Vec::new();
        let mut constructor_body = Vec::new();

        for member in struct_def.members(ast) {
            let identifier = member.identifier();
            let type_node = ast.resolve_index(member.data_type.definition.unwrap());
            let type_string = type_to_string(type_node, ast, TypeContext::DataMember);

            constructor_args.push(format!("{} {}", type_string, identifier));

            constructor_body.push(format!(
                "this.{identifier} = {identifier};",
                identifier = identifier, // TODO: this needs to be split because LHS should use correct case. eg. AnInt = anInt
            ));
        }

        // Write the constructors
        write_fmt!(
            self.output,
            r#"
/// <summary>Constructs a new instance of <see cref="{name}"/>.</summary>{doc_comment}
public {name}({constructor_args})
{{
    {constructor_body}
}}

/// <summary>Constructs a new instance of <see cref="{name}"/> from a decoder.</summary>
public {name}(IceRpc.IceDecoder decoder)
{{
    {decoder_body}
}}"#,
            name = struct_def.identifier(),
            doc_comment = "", //TODO: get doc comment
            constructor_args = constructor_args.join(", "),
            constructor_body = constructor_body.join("\n    "),
            decoder_body = decode_data_members(struct_def.members(ast).as_slice(), ast).indent()
        );

        self.output.write_line_separator();

        write_fmt!(
            self.output,
            "
/// <inheritdoc/>
public readonly override bool Equals(object? obj) => obj is {name} value && this.Equals(value);",
            name = struct_def.identifier()
        );

        if !struct_def.has_attribute("cs:custom-equals") {
            // Default implementation for Equals and GetHashCode.
            self.output.write_line_separator();
            write_fmt!(
                self.output,
                "
/// <inheritdoc/>
public readonly bool Equals({name} other) =>
    {equals};

/// <inheritdoc/>
public readonly override int GetHashCode()
{{
    {hash_code}
}}",
                name = struct_def.identifier(),
                equals = "//TODO: gen equals",
                hash_code = "//TODO: gen hashcode"
            );
        }

        self.output.write_line_separator();
        write_fmt!(
            self.output,
            "
/// <summary>Encodes the fields of this struct.</summary>
public readonly void Encode(IceRpc.IceEncoder encoder)
{{
    {encode_body}
}}",
            encode_body = encode_data_members(struct_def, ast).indent()
        );

        self.output.clear_line_separator();
        self.output.indent_by(-4);
        self.output.write("\n}");
        self.output.write_line_separator();
    }

    fn visit_interface_start(&mut self, interface_def: &Interface, _: usize, _: &Ast) {
        self.write_comment(interface_def);
        let content = format!("\ninterface {}\n{{", interface_def.identifier());
        self.output.write(&content);
        self.output.indent_by(4);
    }

    fn visit_interface_end(&mut self, _: &Interface, _: usize, _: &Ast) {
        self.output.clear_line_separator();
        self.output.indent_by(-4);
        self.output.write("\n}");
        self.output.write_line_separator();
    }

    fn visit_operation_start(&mut self, operation: &Operation, _: usize, ast: &Ast) {
        self.write_comment(operation);
        let mut parameters_string = String::new();
        if !operation.parameters.is_empty() {
            for id in operation.parameters.iter() {
                let parameter = ref_from_node!(Node::Member, ast, *id);
                let data_type = ast.resolve_index(parameter.data_type.definition.unwrap());
                parameters_string += format!(
                    "{} {}, ",
                    type_to_string(data_type, ast, TypeContext::Outgoing),
                    parameter.identifier(),
                )
                .as_str();
            }
            // Remove the trailing comma and space.
            parameters_string.truncate(parameters_string.len() - 2);
        }

        let content = format!(
            "\npublic {} {}({});",
            return_type_to_string(&operation.return_type, ast, TypeContext::Outgoing),
            operation.identifier(),
            parameters_string,
        );
        self.output.write(&content);
        self.output.write_line_separator();
    }

    fn visit_enum_start(&mut self, enum_def: &Enum, _: usize, ast: &Ast) {
        let underlying_type =
            type_to_string(enum_def.underlying_type(ast), ast, TypeContext::Nested);

        self.output.write_line_separator();

        self.write_comment(enum_def);
        //TODO: from slice2cs
        // writeTypeDocComment(p, getDeprecateReason(p));
        // emitCommonAttributes();
        // emitCustomAttributes(p);
        write_fmt!(
            self.output,
            "\npublic enum {name} : {underlying_type}\n{{",
            name = enum_def.identifier(),
            underlying_type = underlying_type
        );
        self.output.indent_by(4);
    }

    fn visit_enum_end(&mut self, enum_def: &Enum, _: usize, ast: &Ast) {
        // Close the enum
        self.output.clear_line_separator();
        self.output.indent_by(-4);
        self.output.write("\n}");
        self.output.write_line_separator();

        let escaped_identifier = escape_identifier(enum_def);

        // When the number of enumerators is smaller than the distance between the min and max values, the values are not
        // consecutive and we need to use a set to validate the value during unmarshaling.
        // Note that the values are not necessarily in order, e.g. we can use a simple range check for
        // enum E { A = 3, B = 2, C = 1 } during unmarshaling.
        let use_set = if let (Some(min_value), Some(max_value)) =
            (enum_def.min_value(ast), enum_def.max_value(ast))
        {
            !enum_def.is_unchecked
                && (enum_def.enumerators.len() as i64) < max_value - min_value + 1
        } else {
            // This means there are no enumerators.*
            true
        };

        let underlying_type =
            type_to_string(enum_def.underlying_type(ast), ast, TypeContext::Nested);

        let hash_set = if use_set {
            format!(
                "\
\npublic static readonly global::System::Collections.Generic.HashSet<{underlying}> EnumeratorValues =
    new global::System.Collections.Generic.HashSet<{underlying}> {{ {enum_values} }}",
                underlying = underlying_type,
                enum_values = enum_def.enumerators(ast).iter().map(|e| e.value.to_string()).collect::<Vec<String>>().join(","))
        } else {
            "".to_owned()
        };

        let as_enum = if enum_def.is_unchecked {
            format!("({})value", escaped_identifier)
        } else {
            let check_enum = if use_set {
                "EnumeratorValues.Contains(value)".to_owned()
            } else {
                //TODO: get the actual min and max values
                format!(
                    "{min_value} <= value && value <= {max_value}",
                    min_value = "min",
                    max_value = "max"
                )
            };
            // TODO: scoped = fixId(p->scoped())
            format!(
                "{check_enum} ? ({escaped_identifier})value : throw new IceRpc.InvalidDataException($\"invalid enumerator value '{{value}}' for {scoped}\")",
                check_enum = check_enum,
                escaped_identifier = escaped_identifier,
                scoped = "...")
        };

        // Enum decoding
        let decode_enum = format!(
            "As{name}(decoder.{decode_method})",
            name = enum_def.identifier(),
            decode_method = if let Some(_) = &enum_def.underlying {
                format!("Decode{}()", builtin_suffix(&enum_def.underlying_type(ast)))
            } else {
                "DecodeSize()".to_owned()
            }
        );

        // Enum encoding
        let encode_enum = if let Some(_) = &enum_def.underlying {
            format!(
                "encoder.Encode{}",
                builtin_suffix(&enum_def.underlying_type(ast))
            )
        } else {
            "encoder.EncodeSize((int)value)".to_owned()
        };

        // Enum helper class
        write_fmt!(
            self.output,
            r#"
/// <summary>Helper class for marshaling and unmarshaling <see cref="{escaped_identifier}"/>.</summary>
public static class {identifier}Helper
{{{hash_set}

    public static {escaped_identifier} As{identifier}(this {underlying_type} value) =>
        {as_enum};

    public static {escaped_identifier} Decode{identifier} (this IceRpc.IceDecoder decoder) =>
        {decode_enum};

    public static void Encode{identifier} (this IceRpc.IceEncoder encoder, {escaped_identifier} value) =>
        {encode_enum};
}}"#,
            escaped_identifier = escaped_identifier,
            identifier = enum_def.identifier(),
            underlying_type = underlying_type,
            hash_set = hash_set.replace("\n", "\n    "),
            as_enum = as_enum.replace("\n", "\n    "),
            decode_enum = decode_enum,
            encode_enum = encode_enum
        );
    }

    fn visit_enumerator(&mut self, enumerator: &Enumerator, _: usize, _: &Ast) {
        self.write_comment(enumerator);
        let content = format!("\n{} = {},", enumerator.identifier(), enumerator.value);
        self.output.write(&content);
    }

    fn visit_data_member(&mut self, data_member: &Member, _: usize, ast: &Ast) {
        self.write_comment(data_member);
        let node = ast.resolve_index(*data_member.data_type.definition.as_ref().unwrap());
        let type_string = type_to_string(node, ast, TypeContext::DataMember);

        let content = format!("\npublic {} {};", type_string, data_member.identifier());
        self.output.write(&content);
    }
}
