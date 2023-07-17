// Copyright (c) ZeroC, Inc.

//! This module pulls in the parsing code generated by LALRPOP and contains private helper functions used by it.
//!
//! While many of these functions could be written directly into the parser rules, we implement them here instead, to
//! keep the rules focused on grammar instead of implementation details, making the grammar easier to read and modify.

use super::parser::Parser;
use crate::ast::node::Node;
use crate::diagnostics::{Diagnostic, Error};
use crate::grammar::*;
use crate::parsers::CommentParser;
use crate::slice_file::Span;
use crate::utils::ptr_util::{OwnedPtr, WeakPtr};
use crate::{downgrade_as, upcast_weak_as};
use lalrpop_util::lalrpop_mod;
use std::num::IntErrorKind;
use std::ops::RangeInclusive;

// Place the code generated by LALRPOP into a submodule named 'lalrpop'.
lalrpop_mod!(
    #[allow(unused, clippy::all)] // LALRPOP generates stuff we don't use, and isn't worth linting.
    pub lalrpop,
    "/parsers/slice/grammar.rs"
);

// This macro does the following:
// 1. Set the parent on each of the children.
// 2. Move the children into the AST and keep pointers to them.
// 3. Store pointers to the children in the parent.
macro_rules! set_children_for_impl {
    ($parent_ptr:expr, $weak_parent_ptr:expr, $children:ident, $parser:expr) => {{
        for mut child in $children {
            unsafe {
                child.borrow_mut().parent = $weak_parent_ptr;
                let weak_ptr = $parser.ast.add_named_element(child);
                $parent_ptr.borrow_mut().$children.push(weak_ptr);
            }
        }
    }};
}

macro_rules! set_children_for {
    ($parent_ptr:expr, $children:ident, $parser:expr) => {
        set_children_for_impl!($parent_ptr, $parent_ptr.downgrade(), $children, $parser);
    };
}

macro_rules! set_fields_for {
    ($parent_ptr:expr, $children:ident, $parser:expr) => {{
        let downgraded = downgrade_as!($parent_ptr, dyn Container<WeakPtr<Field>>);
        set_children_for_impl!($parent_ptr, downgraded.clone(), $children, $parser);
    }};
}

// Convenience type for storing an unparsed doc comment. Each element of the vector is one line of the comment.
type RawDocComment<'a> = Vec<(&'a str, Span)>;

// Grammar Rule Functions

fn handle_file_compilation_mode(
    parser: &mut Parser,
    (old_mode, attributes): (Option<FileCompilationMode>, Vec<WeakPtr<Attribute>>),
    mode: FileCompilationMode,
) -> (Option<FileCompilationMode>, Vec<WeakPtr<Attribute>>) {
    // Compilation mode can only be set once per file.
    if let Some(old_file_mode) = old_mode {
        let old_span = old_file_mode.span();
        let diagnostic = Diagnostic::new(Error::MultipleCompilationModes)
            .set_span(old_span)
            .add_note("the compilation mode was previously specified here", Some(old_span));
        parser.diagnostics.push(diagnostic);
    }
    parser.compilation_mode = mode.version;
    (Some(mode), attributes)
}

fn construct_file_compilation_mode(parser: &mut Parser, i: Identifier, span: Span) -> FileCompilationMode {
    let version = match i.value.as_str() {
        "Slice1" => CompilationMode::Slice1,
        "Slice2" => CompilationMode::Slice2,
        _ => {
            let diagnostic = Diagnostic::new(Error::InvalidCompilationMode { mode: i.value })
                .set_span(&i.span)
                .add_note("must be 'Slice1' or 'Slice2'", None);
            parser.diagnostics.push(diagnostic);
            CompilationMode::default() // Dummy
        }
    };
    FileCompilationMode { version, span }
}

fn construct_module(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    span: Span,
) -> OwnedPtr<Module> {
    if !raw_comment.is_empty() {
        let error = Error::Syntax {
            message: "doc comments cannot be applied to modules".to_owned(),
        };
        parser.diagnostics.push(Diagnostic::new(error).set_span(&span));
    }

    let module_ptr = OwnedPtr::new(Module {
        identifier,
        attributes,
        span,
    });

    parser.current_scope.module = Some(module_ptr.downgrade());
    parser.current_scope.parser_scope = module_ptr.borrow().nested_module_identifier().to_owned();
    module_ptr
}

fn construct_struct(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    is_compact: bool,
    identifier: Identifier,
    fields: Vec<OwnedPtr<Field>>,
    span: Span,
) -> OwnedPtr<Struct> {
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);
    let mut struct_ptr = OwnedPtr::new(Struct {
        identifier,
        fields: Vec::new(),
        is_compact,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    });

    // Add all the fields to the struct.
    set_fields_for!(struct_ptr, fields, parser);

    struct_ptr
}

fn construct_exception(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    base_type: Option<TypeRef>,
    fields: Vec<OwnedPtr<Field>>,
    span: Span,
) -> OwnedPtr<Exception> {
    let base = base_type.map(|type_ref| type_ref.downcast::<Exception>().unwrap());
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    let mut exception_ptr = OwnedPtr::new(Exception {
        identifier,
        fields: Vec::new(),
        base,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    });

    // Add all the fields to the exception.
    set_fields_for!(exception_ptr, fields, parser);

    exception_ptr
}

fn construct_class(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    compact_id: Option<Integer<u32>>,
    base_type: Option<TypeRef>,
    fields: Vec<OwnedPtr<Field>>,
    span: Span,
) -> OwnedPtr<Class> {
    let base = base_type.map(|type_ref| type_ref.downcast::<Class>().unwrap());
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    let mut class_ptr = OwnedPtr::new(Class {
        identifier,
        fields: Vec::new(),
        compact_id,
        base,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    });

    // Add all the fields to the class.
    set_fields_for!(class_ptr, fields, parser);

    class_ptr
}

pub fn construct_field(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    tag: Option<Integer<u32>>,
    data_type: TypeRef,
    span: Span,
) -> OwnedPtr<Field> {
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);
    OwnedPtr::new(Field {
        identifier,
        data_type,
        tag,
        parent: WeakPtr::create_uninitialized(), // Patched by its container.
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
    })
}

fn construct_interface(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    bases: Option<Vec<TypeRef>>,
    operations: Vec<OwnedPtr<Operation>>,
    span: Span,
) -> OwnedPtr<Interface> {
    let bases = bases
        .unwrap_or_default() // Create an empty vector if no bases were specified.
        .into_iter()
        .map(|base| base.downcast::<Interface>().unwrap())
        .collect::<Vec<_>>();
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    let mut interface_ptr = OwnedPtr::new(Interface {
        identifier,
        operations: Vec::new(),
        bases,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    });

    // Add all the operations to the interface.
    set_children_for!(interface_ptr, operations, parser);

    interface_ptr
}

#[allow(clippy::too_many_arguments)]
fn construct_operation(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    is_idempotent: bool,
    identifier: Identifier,
    parameters: Vec<OwnedPtr<Parameter>>,
    return_type: Option<Vec<OwnedPtr<Parameter>>>,
    exception_specification: Option<Throws>,
    span: Span,
) -> OwnedPtr<Operation> {
    // If no return type was provided set the return type to an empty Vec.
    let return_type = return_type.unwrap_or_default();

    // If no throws clause was present, set the exception specification to `None`.
    let throws = exception_specification.unwrap_or(Throws::None);

    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    let mut operation_ptr = OwnedPtr::new(Operation {
        identifier,
        parameters: Vec::new(),
        return_type: Vec::new(),
        throws,
        is_idempotent,
        encoding: parser.compilation_mode,
        parent: WeakPtr::create_uninitialized(), // Patched by its container.
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
    });

    // Add all the parameters and return members to the operation.
    set_children_for!(operation_ptr, parameters, parser);
    set_children_for!(operation_ptr, return_type, parser);

    operation_ptr
}

#[allow(clippy::too_many_arguments)]
fn construct_parameter(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    tag: Option<Integer<u32>>,
    is_streamed: bool,
    data_type: TypeRef,
    span: Span,
    is_returned: bool,
) -> OwnedPtr<Parameter> {
    if !raw_comment.is_empty() {
        let kind = match is_returned {
            true => "return member",
            false => "parameter",
        };
        let diagnostic = Diagnostic::new(Error::Syntax {
            message: format!("doc comments cannot be applied to {kind}s"),
        })
        .set_span(&span)
        .add_note("try using an '@param' tag on the operation it belongs to instead", None)
        .add_note(format!("Ex: @param {}: {}", &identifier.value, raw_comment[0].0), None);
        parser.diagnostics.push(diagnostic);
    }

    OwnedPtr::new(Parameter {
        identifier,
        data_type,
        tag,
        is_streamed,
        is_returned,
        parent: WeakPtr::create_uninitialized(), // Patched by its container.
        scope: parser.current_scope.clone(),
        attributes,
        span,
    })
}

fn construct_single_return_type(
    parser: &Parser,
    tag: Option<Integer<u32>>,
    is_streamed: bool,
    data_type: TypeRef,
    span: Span,
) -> Vec<OwnedPtr<Parameter>> {
    // Create a dummy identifier for the return type, since it's nameless.
    let dummy_identifier = Identifier {
        value: "returnValue".to_owned(),
        span: span.clone(),
    };

    vec![OwnedPtr::new(Parameter {
        identifier: dummy_identifier,
        data_type,
        tag,
        is_streamed,
        is_returned: true,
        parent: WeakPtr::create_uninitialized(), // Patched by its container.
        scope: parser.current_scope.clone(),
        attributes: Vec::new(),
        span,
    })]
}

fn check_return_tuple(parser: &mut Parser, return_tuple: &Vec<OwnedPtr<Parameter>>, span: Span) {
    if return_tuple.len() < 2 {
        let diagnostic = Diagnostic::new(Error::ReturnTuplesMustContainAtLeastTwoElements).set_span(&span);
        parser.diagnostics.push(diagnostic);
    }
}

fn construct_enum(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    is_unchecked: bool,
    identifier: Identifier,
    underlying_type: Option<TypeRef>,
    enumerators: Vec<OwnedPtr<Enumerator>>,
    span: Span,
) -> OwnedPtr<Enum> {
    let underlying = underlying_type.map(|type_ref| type_ref.downcast::<Primitive>().unwrap());
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    let mut enum_ptr = OwnedPtr::new(Enum {
        identifier,
        enumerators: Vec::new(),
        underlying,
        is_unchecked,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    });

    // Add all the enumerators to the enum.
    set_children_for!(enum_ptr, enumerators, parser);

    // Clear the `last_enumerator_value` field since this is the end of the enum.
    parser.last_enumerator_value = None;

    enum_ptr
}

fn construct_enumerator(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    enumerator_value: Option<Integer<i128>>,
    span: Span,
) -> OwnedPtr<Enumerator> {
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);

    // If the enumerator was given an explicit value, use it. Otherwise it's implicit value is calculated as follows:
    // If this is the first enumerator in the enum its value is 0 (since `last_enumerator_value` is `None`).
    // For any other enumerator, its value is equal to the previous enumerator's value plus 1.
    let value = match enumerator_value {
        Some(integer) => EnumeratorValue::Explicit(integer),
        None => EnumeratorValue::Implicit(parser.last_enumerator_value.map_or(0, |x| x.wrapping_add(1))),
    };

    let enumerator = OwnedPtr::new(Enumerator {
        identifier,
        value,
        parent: WeakPtr::create_uninitialized(), // Patched by its container.
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
    });

    parser.last_enumerator_value = Some(enumerator.borrow().value());
    enumerator
}

fn construct_custom_type(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    span: Span,
) -> OwnedPtr<CustomType> {
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);
    OwnedPtr::new(CustomType {
        identifier,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    })
}

fn construct_type_alias(
    parser: &mut Parser,
    (raw_comment, attributes): (RawDocComment, Vec<WeakPtr<Attribute>>),
    identifier: Identifier,
    underlying: TypeRef,
    span: Span,
) -> OwnedPtr<TypeAlias> {
    let comment = parse_doc_comment(parser, &identifier.value, raw_comment);
    OwnedPtr::new(TypeAlias {
        identifier,
        underlying,
        scope: parser.current_scope.clone(),
        attributes,
        comment,
        span,
        supported_encodings: None, // Patched by the encoding patcher.
    })
}

fn construct_type_ref(
    parser: &Parser,
    attributes: Vec<WeakPtr<Attribute>>,
    definition: TypeRefDefinition,
    is_optional: bool,
    span: Span,
) -> TypeRef {
    TypeRef {
        definition,
        is_optional,
        scope: parser.current_scope.clone(),
        attributes,
        span,
    }
}

fn primitive_to_type_ref_definition(parser: &Parser, primitive: Primitive) -> TypeRefDefinition {
    // These unwraps are safe because the primitive types are always defined in the AST.
    let node = parser.ast.find_node(primitive.kind()).unwrap();
    let weak_ptr: WeakPtr<Primitive> = node.try_into().unwrap();
    TypeRefDefinition::Patched(upcast_weak_as!(weak_ptr, dyn Type))
}

fn anonymous_type_to_type_ref_definition<T>(parser: &mut Parser, ptr: OwnedPtr<T>) -> TypeRefDefinition
where
    T: Type + 'static,
    OwnedPtr<T>: Into<Node>,
{
    let weak_ptr = parser.ast.add_element(ptr);
    TypeRefDefinition::Patched(upcast_weak_as!(weak_ptr, dyn Type))
}

fn construct_unpatched_type_ref_definition(mut identifier: Identifier) -> TypeRefDefinition {
    // Remove any whitespace from the identifier so it can be looked up in the AST.
    identifier.value.retain(|c| !c.is_whitespace());
    TypeRefDefinition::Unpatched(identifier)
}

fn construct_attribute(
    parser: &mut Parser,
    directive: Identifier,
    arguments: Option<Vec<String>>,
    span: Span,
) -> WeakPtr<Attribute> {
    let attribute = Attribute::new(directive.value, arguments.unwrap_or_default(), span);
    parser.ast.add_element(OwnedPtr::new(attribute))
}

fn try_parse_integer(parser: &mut Parser, s: &str, span: Span) -> Integer<i128> {
    // Remove any underscores from the integer literal before trying to parse it.
    let sanitized = s.replace('_', "");

    // Check the literal for a base prefix. If present, remove it and set the base.
    // "0b" = binary, "0x" = hexadecimal, otherwise we assume it's decimal.
    let (literal, base) = match sanitized {
        _ if sanitized.starts_with("0b") => (&sanitized[2..], 2),
        _ if sanitized.starts_with("0x") => (&sanitized[2..], 16),
        _ => (sanitized.as_str(), 10),
    };

    let value = match i128::from_str_radix(literal, base) {
        Ok(x) => x,
        Err(err) => {
            let e = match err.kind() {
                IntErrorKind::InvalidDigit => Error::InvalidIntegerLiteral { base },
                _ => Error::IntegerLiteralOverflows,
            };
            parser.diagnostics.push(Diagnostic::new(e).set_span(&span));
            0 // Dummy value
        }
    };

    Integer { value, span }
}

fn parse_tag_value(parser: &mut Parser, i: Integer<i128>) -> Integer<u32> {
    // Verify that the provided integer is a valid tag id.
    if !RangeInclusive::new(0, i32::MAX as i128).contains(&i.value) {
        let diagnostic = Diagnostic::new(Error::TagValueOutOfBounds).set_span(&i.span);
        parser.diagnostics.push(diagnostic);
    }

    // Cast the integer to a `u32` since it most closely matches the allowed range of tags.
    // It's fine if the value doesn't fit, the cast will just give us a dummy value.
    let value = i.value as u32;
    Integer { value, span: i.span }
}

fn parse_compact_id_value(parser: &mut Parser, i: Integer<i128>) -> Integer<u32> {
    // Verify that the provided integer is a valid compact id.
    if !RangeInclusive::new(0, i32::MAX as i128).contains(&i.value) {
        let diagnostic = Diagnostic::new(Error::CompactIdOutOfBounds).set_span(&i.span);
        parser.diagnostics.push(diagnostic);
    }

    // Cast the integer to a `u32` since it most closely matches the allowed range of compact ids.
    // It's fine if the value doesn't fit, the cast will just give us a dummy value.
    let value = i.value as u32;
    Integer { value, span: i.span }
}

fn parse_doc_comment(parser: &mut Parser, identifier: &str, raw_comment: RawDocComment) -> Option<DocComment> {
    if raw_comment.is_empty() {
        // If the doc comment had 0 lines, that just means there is no doc comment.
        None
    } else {
        let scoped_identifier = get_scoped_identifier(identifier, &parser.current_scope.parser_scope);
        let comment_parser = CommentParser::new(parser.file_name, &scoped_identifier, parser.diagnostics);
        comment_parser.parse_doc_comment(raw_comment).ok()
    }
}
