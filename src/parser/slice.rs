
use super::comments::CommentParser;
use crate::ast::Ast;
use crate::grammar::*;
use crate::ptr_util::{OwnedPtr, WeakPtr};
use crate::slice_file::{Location, SliceFile};
use crate::upcast_weak_as;
use std::cell::RefCell;
use std::default::Default;
use std::fs;

use pest::error::ErrorVariant as PestErrorVariant;
use pest_consume::{match_nodes, Error as PestError, Parser as PestParser};

type PestResult<T> = Result<T, PestError<Rule>>;
type PestNode<'a, 'b, 'ast> = pest_consume::Node<'a, Rule, &'b RefCell<ParserData<'ast>>>;

fn from_span(input: &PestNode) -> Location {
    let span = input.as_span();
    Location {
        start: span.start_pos().line_col(),
        end: span.end_pos().line_col(),
        file: input.user_data().borrow().current_file.clone(),
    }
}

fn get_scope(input: &PestNode) -> Scope {
    input.user_data().borrow().current_scope.clone()
}

fn push_scope(input: &PestNode, identifier: &str, is_module: bool) {
    let scope = &mut input.user_data().borrow_mut().current_scope;
    scope.push_scope(identifier, is_module);
}

fn pop_scope(input: &PestNode) {
    let scope = &mut input.user_data().borrow_mut().current_scope;
    scope.pop_scope();
}

#[derive(Debug)]
struct ParserData<'ast> {
    ast: &'ast mut Ast,
    current_file: String,
    current_enum_value: i64,
    current_scope: Scope,
}

#[derive(PestParser)]
#[grammar = "parser/slice.pest"]
pub(super) struct SliceParser;

impl SliceParser {
    pub fn try_parse_file(&self, file: &str, is_source: bool, ast: &mut Ast) -> Option<SliceFile> {
        match self.parse_file(file, is_source, ast) {
            Ok(slice_file) => {
                Some(slice_file)
            }
            Err(message) => {
                crate::report_error(message, None);
                None
            }
        }
    }

    fn parse_file(&self, file: &str, is_source: bool, ast: &mut Ast) -> Result<SliceFile, String> {
        let user_data = RefCell::new(ParserData {
            ast,
            current_file: file.to_owned(),
            current_enum_value: 0,
            current_scope: Scope::default(),
        });

        // Read the raw text from the file, and parse it into a raw ast.
        let raw_text = fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let node = SliceParser::parse_with_userdata(Rule::main, &raw_text, &user_data)
            .map_err(|e| e.to_string())?; // TODO maybe make this error print prettier?
        let raw_ast = node.single().expect("Failed to unwrap raw_ast!");

        // Consume the raw ast into an unpatched ast, then store it in a `SliceFile`.
        let (file_attributes, file_contents) =
            SliceParser::main(raw_ast).map_err(|e| e.to_string())?;

        let top_level_modules = file_contents.into_iter().map(|module_def| {
            ast.add_module(module_def)
        }).collect::<Vec<_>>();

        Ok(SliceFile::new(
            file.to_owned(),
            raw_text,
            top_level_modules,
            file_attributes,
            is_source,
        ))
    }
}

#[pest_consume::parser]
impl SliceParser {
    fn main(input: PestNode) -> PestResult<(Vec<Attribute>, Vec<Module>)> {
        let module_ids = match_nodes!(input.into_children();
            [file_attributes(attributes), module_def(modules).., EOI(_)] => {
                (attributes, modules.collect())
            }
        );
        Ok(module_ids)
    }

    fn definition(input: PestNode) -> PestResult<Definition> {
        Ok(match_nodes!(input.into_children();
            [module_def(module_def)]       => Definition::Module(OwnedPtr::new(module_def)),
            [struct_def(struct_def)]       => Definition::Struct(OwnedPtr::new(struct_def)),
            [class_def(class_def)]         => Definition::Class(OwnedPtr::new(class_def)),
            [exception_def(exception_def)] => Definition::Exception(OwnedPtr::new(exception_def)),
            [interface_def(interface_def)] => Definition::Interface(OwnedPtr::new(interface_def)),
            [enum_def(enum_def)]           => Definition::Enum(OwnedPtr::new(enum_def)),
            [type_alias(type_alias)]       => Definition::TypeAlias(OwnedPtr::new(type_alias)),
        ))
    }

    fn module_start(input: PestNode) -> PestResult<(Identifier, Location)> {
        let location = from_span(&input);
        let identifier = match_nodes!(input.children();
            [_, scoped_identifier(ident)] => ident,
        );

        // Split the identifier in case it uses nested module syntax, and push a scope for each.
        for module_identifier in identifier.value.split("::") {
            push_scope(&input, module_identifier, true);
        }
        Ok((identifier, location))
    }

    fn module_def(input: PestNode) -> PestResult<Module> {
        Ok(match_nodes!(input.children();
            [prelude(prelude), module_start(module_start), definition(definitions)..] => {
                let (identifier, location) = module_start;
                let (attributes, comment) = prelude;

                // Split the identifier in case it uses nested module syntax.
                // We iterate in reverse, since we construct them in inner-to-outermost order.
                let mut modules = identifier.value.rsplit("::");

                // Pop the scope of the inner-most module (the module can't be in its own scope).
                pop_scope(&input);
                // Construct the inner-most module first.
                let mut last_module = Module::new(
                    // There must be at least one module identifier, so it's safe to unwrap here.
                    Identifier::new(
                        modules.next().unwrap().to_owned(),
                        identifier.location.clone(),
                    ),
                    get_scope(&input),
                    attributes,
                    comment,
                    location.clone(),
                );
                // Add the definitions into the inner-most module.
                for definition in definitions {
                    last_module.add_definition(definition);
                }

                // Construct any enclosing modules.
                for module in modules {
                    // Pop the module's scope, and then construct it.
                    pop_scope(&input);
                    let mut new_module = Module::new(
                        Identifier::new(module.to_owned(), identifier.location.clone()),
                        get_scope(&input),
                        Vec::new(),
                        None,
                        location.clone(),
                    );
                    // Add the inner module to the outer module, than swap their variables.
                    new_module.add_definition(Definition::Module(OwnedPtr::new(last_module)));
                    last_module = new_module;
                }

                // Return the outer-most module.
                last_module
            },
        ))
    }

    fn struct_start(input: PestNode) -> PestResult<(Identifier, Location)> {
        let location = from_span(&input);
        let identifier = match_nodes!(input.children();
            [_, identifier(ident)] => ident,
        );
        push_scope(&input, &identifier.value, false);
        Ok((identifier, location))
    }

    fn struct_def(input: PestNode) -> PestResult<Struct> {
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), struct_start(struct_start), data_member(members)..] => {
                let (identifier, location) = struct_start;
                let (attributes, comment) = prelude;
                let mut struct_def = Struct::new(identifier, scope, attributes, comment, location);
                for member in members {
                    struct_def.add_member(member);
                }
                pop_scope(&input);
                struct_def
            },
        ))
    }

    fn class_start(input: PestNode) -> PestResult<(Identifier, Option<u32>, Location, Option<TypeRef<Class>>)> {
        let location = from_span(&input);
        Ok(match_nodes!(input.children();
            [_, identifier(identifier), compact_id(compact_id)] => {
                push_scope(&input, &identifier.value, false);
                (identifier, compact_id, location, None)
            },
            [_, identifier(identifier), compact_id(compact_id), _, inheritance_list(bases)] => {
                // Classes can only inherit from a single base class.
                if bases.len() > 1 {
                    //TODO let error_handler = &mut input.user_data().borrow_mut().error_handler;
                    //TODO error_handler.report_error((
                    //TODO     format!("classes can only inherit from a single base class"),
                    //TODO     location.clone()
                    //TODO ).into());
                }

                push_scope(&input, &identifier.value, false);

                let base = bases.into_iter().next().unwrap().downcast::<Class>().unwrap();
                (identifier, compact_id, location, Some(base))
            }
        ))
    }

    fn class_def(input: PestNode) -> PestResult<Class> {
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), class_start(class_start), data_member(members)..] => {
                let (identifier, compact_id, location, base) = class_start;
                let (attributes, comment) = prelude;
                let mut class = Class::new(identifier, compact_id, base, scope, attributes, comment, location);
                for member in members {
                    class.add_member(member);
                }
                pop_scope(&input);
                class
            },
        ))
    }

    fn exception_start(input: PestNode) -> PestResult<(Identifier, Location, Option<TypeRef<Exception>>)> {
        let location = from_span(&input);
        Ok(match_nodes!(input.children();
            [_, identifier(identifier)] => {
                push_scope(&input, &identifier.value, false);
                (identifier, location, None)
            },
            [_, identifier(identifier), _, inheritance_list(bases)] => {
                // Exceptions can only inherit from a single base exception.
                if bases.len() > 1 {
                    //TODO let error_handler = &mut input.user_data().borrow_mut().error_handler;
                    //TODO error_handler.report_error((
                    //TODO     format!("exceptions can only inherit from a single base exception"),
                    //TODO     location.clone()
                    //TODO ).into());
                }

                push_scope(&input, &identifier.value, false);

                let base = bases.into_iter().next().unwrap().downcast::<Exception>().unwrap();
                (identifier, location, Some(base))
            }
        ))
    }

    fn exception_def(input: PestNode) -> PestResult<Exception> {
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), exception_start(exception_start), data_member(members)..] => {
                let (identifier, location, base) = exception_start;
                let (attributes, comment) = prelude;
                let mut exception = Exception::new(identifier, base, scope, attributes, comment, location);
                for member in members {
                    exception.add_member(member);
                }
                pop_scope(&input);
                exception
            },
        ))
    }

    fn interface_start(input: PestNode) -> PestResult<(Identifier, Location, Vec<TypeRef<Interface>>)> {
        let location = from_span(&input);
        Ok(match_nodes!(input.children();
            [_, identifier(identifier)] => {
                push_scope(&input, &identifier.value, false);
                (identifier, location, Vec::new())
            },
            [_, identifier(identifier), _, inheritance_list(bases)] => {
                let mut bases_vector = Vec::new();
                for base in bases {
                    bases_vector.push(base.downcast::<Interface>().unwrap());
                }
                push_scope(&input, &identifier.value, false);
                (identifier, location, bases_vector)
            }
        ))
    }

    fn interface_def(input: PestNode) -> PestResult<Interface> {
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), interface_start(interface_start), operation(operations)..] => {
                let (identifier, location, bases) = interface_start;
                let (attributes, comment) = prelude;
                let mut interface = Interface::new(
                    identifier,
                    bases,
                    scope,
                    attributes,
                    comment,
                    location,
                );
                for operation in operations {
                    interface.add_operation(operation);
                }
                pop_scope(&input);
                interface
            },
        ))
    }

    fn enum_start(input: PestNode) -> PestResult<(bool, Identifier, Location, Option<TypeRef<Primitive>>)> {
        // Reset the current enumerator value back to 0.
        input.user_data().borrow_mut().current_enum_value = 0;

        let location = from_span(&input);
        Ok(match_nodes!(input.children();
            [unchecked_modifier(unchecked), _, identifier(identifier)] => {
                push_scope(&input, &identifier.value, false);
                (unchecked, identifier, location, None)
            },
            [unchecked_modifier(unchecked), _, identifier(identifier), _, typeref(type_ref)] => {
                let underlying = match type_ref.downcast::<Primitive>() {
                    Ok(primtive_def) => primtive_def,
                    _ => panic!("MUST BE A PRIMITIVE TODO"),
                };
                push_scope(&input, &identifier.value, false);
                (unchecked, identifier, location, Some(underlying))
            },
        ))
    }

    fn enum_def(input: PestNode) -> PestResult<Enum> {
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), enum_start(enum_start), enumerator_list(enumerators)] => {
                let (is_unchecked, identifier, location, underlying) = enum_start;
                let (attributes, comment) = prelude;
                let mut enum_def = Enum::new(
                    identifier,
                    underlying,
                    is_unchecked,
                    scope,
                    attributes,
                    comment,
                    location,
                );
                for enumerator in enumerators {
                    enum_def.add_enumerator(enumerator);
                }
                pop_scope(&input);
                enum_def
            },
            [prelude(prelude), enum_start(enum_start)] => {
                let (is_unchecked, identifier, location, underlying) = enum_start;
                let (attributes, comment) = prelude;
                pop_scope(&input);
                Enum::new(
                    identifier,
                    underlying,
                    is_unchecked,
                    scope,
                    attributes,
                    comment,
                    location,
                )
            },
        ))
    }

    // Parses an operation's return type. There are 3 possible syntaxes for a return type:
    //   A void return type, specified by the `void` keyword.
    //   A single unnamed return type, specified by a typename.
    //   A return tuple, specified as a list of named elements enclosed in parenthesis.
    fn return_type(input: PestNode) -> PestResult<Vec<OwnedPtr<Parameter>>> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [void_kw(_)] => Vec::new(),
            [return_tuple(tuple)] => tuple,
            [stream_modifier(is_streamed), typeref(data_type)] => {
                let identifier = Identifier::new("".to_owned(), location.clone());
                vec![OwnedPtr::new(Parameter::new(
                    identifier,
                    data_type,
                    None,
                    is_streamed,
                    true,
                    scope,
                    Vec::new(),
                    None,
                    location,
                ))]
            },
            [stream_modifier(is_streamed), tag(tag), typeref(data_type)] => {
                let identifier = Identifier::new("".to_owned(), location.clone());
                vec![OwnedPtr::new(Parameter::new(
                    identifier,
                    data_type,
                    Some(tag),
                    is_streamed,
                    true,
                    scope,
                    Vec::new(),
                    None,
                    location,
                ))]
            },
        ))
    }

    // Parses a return type that is written in return tuple syntax.
    fn return_tuple(input: PestNode) -> PestResult<Vec<OwnedPtr<Parameter>>> {
        // TODO we need to enforce there being more than 1 element here!
        Ok(match_nodes!(input.children();
            // Return tuple elements and parameters have the same syntax, so we re-use the parsing
            // for parameter lists, then change their member type here, after the fact.
            [parameter_list(return_elements)] => {
                return_elements.into_iter().map(
                    |mut parameter| { parameter.is_returned = true; OwnedPtr::new(parameter) }
                ).collect::<Vec<_>>()
            },
        ))
    }

    fn operation_start(input: PestNode) -> PestResult<(bool, Vec<OwnedPtr<Parameter>>, Identifier)> {
        Ok(match_nodes!(input.children();
            [idempotent_modifier(is_idempotent), return_type(return_type), identifier(identifier)] => {
                push_scope(&input, &identifier.value, false);
                (is_idempotent, return_type, identifier)
            }
        ))
    }

    fn operation(input: PestNode) -> PestResult<Operation> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        let mut operation = match_nodes!(input.children();
            [prelude(prelude), operation_start(operation_start)] => {
                let (attributes, comment) = prelude;
                let (is_idempotent, return_type, identifier) = operation_start;
                pop_scope(&input);
                Operation::new(identifier, return_type, is_idempotent, scope, attributes, comment, location)
            },
            [prelude(prelude), operation_start(operation_start), parameter_list(parameters)] => {
                let (attributes, comment) = prelude;
                let (is_idempotent, return_type, identifier) = operation_start;
                let mut operation = Operation::new(identifier, return_type, is_idempotent, scope, attributes, comment, location);
                for parameter in parameters {
                    operation.add_parameter(parameter);
                }
                pop_scope(&input);
                operation
            },
        );

        // Forward the operations's attributes to the return type, if it returns a single type.
        // TODO: in the future we should only forward type metadata by filtering metadata.
        if operation.return_type.len() == 1 {
            // TODO don't do this.
            unsafe { operation.return_type[0].borrow_mut().attributes = operation.attributes.clone(); }
        }
        Ok(operation)
    }

    fn data_member(input: PestNode) -> PestResult<DataMember> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), member(member)] => {
                let (attributes, comment) = prelude;
                let (tag, mut data_type, identifier) = member;

                // Forward the member's attributes to the data type.
                // TODO: in the future we should only forward type metadata by filtering metadata.
                data_type.attributes = attributes.clone();

                DataMember::new(
                    identifier,
                    data_type,
                    tag,
                    scope,
                    attributes,
                    comment,
                    location,
                )
            },
        ))
    }

    fn member(input: PestNode) -> PestResult<(Option<u32>, TypeRef, Identifier)> {
        Ok(match_nodes!(input.into_children();
            [tag(tag), typeref(data_type), identifier(identifier)] => {
                (Some(tag), data_type, identifier)
            },
            [typeref(data_type), identifier(identifier)] => {
                (None, data_type, identifier)
            }
        ))
    }

    fn tag(input: PestNode) -> PestResult<u32> {
        Ok(match_nodes!(input.children();
            [_, integer(integer)] => {
                // tags must fit in an i32 and be non-negative.
                if integer < 0 || integer > i32::MAX.into() {
                    // TODO let location = from_span(&input);
                    // TODO let error_string = if integer < 0 {
                    // TODO     format!("tag is out of range: {}. Tag values must be positive", integer)
                    // TODO } else {
                    // TODO     format!(
                    // TODO         "tag is out of range: {}. Tag values must be less than {}",
                    // TODO         integer, i32::MAX
                    // TODO     )
                    // TODO };
                    // TODO report an error here!
                }
                integer as u32
            }
        ))
    }

    fn parameter_list(input: PestNode) -> PestResult<Vec<Parameter>> {
        Ok(match_nodes!(input.into_children();
            [parameter(parameter)] => {
                vec![parameter]
            },
            [parameter(parameter), parameter_list(mut list)] => {
                // The parameter comes before the parameter_list when parsing, so we have to
                // insert the new parameter at the front of the list.
                list.insert(0, parameter);
                list
            },
        ))
    }

    fn parameter(input: PestNode) -> PestResult<Parameter> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), stream_modifier(is_streamed), member(member)] => {
                let (attributes, comment) = prelude;
                let (tag, mut data_type, identifier) = member;

                // Forward the member's attributes to the data type.
                // TODO: in the future we should only forward type metadata by filtering metadata.
                data_type.attributes = attributes.clone();

                Parameter::new(
                    identifier,
                    data_type,
                    tag,
                    is_streamed,
                    false,
                    scope,
                    attributes,
                    comment,
                    location,
                )
            },
        ))
    }

    fn enumerator_list(input: PestNode) -> PestResult<Vec<Enumerator>> {
        Ok(match_nodes!(input.into_children();
            [enumerator(enumerator)] => {
                vec![enumerator]
            },
            [enumerator(enumerator), enumerator_list(mut list)] => {
                // The enumerator comes before the enumerator_list when parsing, so we have to
                // insert the new enumerator at the front of the list.
                list.insert(0, enumerator);
                list
            },
        ))
    }

    fn enumerator(input: PestNode) -> PestResult<Enumerator> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        let mut next_enum_value = input.user_data().borrow().current_enum_value;

        let enumerator = match_nodes!(input.children();
            [prelude(prelude), identifier(ident)] => {
                let (attributes, comment) = prelude;
                Enumerator::new(ident, next_enum_value, scope, attributes, comment, location)
            },
            [prelude(prelude), identifier(ident), integer(value)] => {
                next_enum_value = value;
                let (attributes, comment) = prelude;
                Enumerator::new(ident, value, scope, attributes, comment, location)
            },
        );

        let parser_data = &mut input.user_data().borrow_mut();
        parser_data.current_enum_value = next_enum_value + 1;
        Ok(enumerator)
    }

    fn inheritance_list(input: PestNode) -> PestResult<Vec<TypeRef>> {
        Ok(match_nodes!(input.into_children();
            [typeref(typeref)] => {
                vec![typeref]
            },
            [typeref(typeref), inheritance_list(mut list)] => {
                // The typename comes before the inheritance_list when parsing, so we have to
                // insert the new typename at the front of the list.
                list.insert(0, typeref);
                list
            },
        ))
    }

    fn type_alias(input: PestNode) -> PestResult<TypeAlias> {
        let location = from_span(&input);
        let scope = get_scope(&input);
        Ok(match_nodes!(input.children();
            [prelude(prelude), _, identifier(identifier), typeref(type_ref)] => {
                let (attributes, comment) = prelude;
                TypeAlias::new(identifier, type_ref, scope, attributes, comment, location)
            },
        ))
    }

    fn typeref(input: PestNode) -> PestResult<TypeRef> {
        let location = from_span(&input);
        let mut nodes = input.children();

        // The first node is always a `local_attribute`. This is guaranteed by the grammar rules.
        let attributes = SliceParser::local_attributes(nodes.next().unwrap()).unwrap();
        // The second node is the type.
        let type_node = nodes.next().unwrap();

        // Get the typename as a string, with any whitespace removed from it.
        let type_name = type_node
            .as_str()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        let is_optional = input.as_str().ends_with('?');
        let scope = get_scope(&input);
        let mut type_ref: TypeRef<dyn Type> =
            TypeRef::new(type_name, is_optional, scope, attributes, location);

        // Resolve and/or construct non user defined types.
        match type_node.as_rule() {
            Rule::primitive => {
                type_ref.definition =
                    upcast_weak_as!(Self::primitive(type_node).unwrap(), dyn Type);
            }
            Rule::sequence => {
                // Store the sequence in the AST's anonymous types vector.
                let sequence = Self::sequence(type_node).unwrap();
                let ast = &mut input.user_data().borrow_mut().ast;
                type_ref.definition = ast.add_anonymous_type(sequence).downgrade();
            }
            Rule::dictionary => {
                // Store the dictionary in the AST's anonymous types vector.
                let dictionary = Self::dictionary(type_node).unwrap();
                let ast = &mut input.user_data().borrow_mut().ast;
                type_ref.definition = ast.add_anonymous_type(dictionary).downgrade();
            }
            // Nothing to do, we wait until after we've generated a lookup table to patch user
            // defined types.
            _ => {}
        }
        Ok(type_ref)
    }

    fn sequence(input: PestNode) -> PestResult<Sequence> {
        Ok(match_nodes!(input.into_children();
            [_, typeref(element_type)] => {
                Sequence { element_type }
            },
        ))
    }

    fn dictionary(input: PestNode) -> PestResult<Dictionary> {
        Ok(match_nodes!(input.into_children();
            [_, typeref(key_type), typeref(value_type)] => {
                Dictionary { key_type, value_type }
            },
        ))
    }

    fn primitive(input: PestNode) -> PestResult<WeakPtr<Primitive>> {
        // Look the primitive up in the AST's primitive cache.
        Ok(input.user_data().borrow().ast.lookup_primitive(input.as_str()).downgrade())
    }

    fn identifier(input: PestNode) -> PestResult<Identifier> {
        Ok(Identifier::new(input.as_str().to_owned(), from_span(&input)))
    }

    fn scoped_identifier(input: PestNode) -> PestResult<Identifier> {
        Ok(Identifier::new(input.as_str().to_owned(), from_span(&input)))
    }

    fn global_identifier(input: PestNode) -> PestResult<Identifier> {
        Ok(Identifier::new(input.as_str().to_owned(), from_span(&input)))
    }

    fn prelude(input: PestNode) -> PestResult<(Vec<Attribute>, Option<DocComment>)> {
        Ok(match_nodes!(input.into_children();
            [local_attributes(mut attributes1), doc_comment(comment), local_attributes(attributes2)] => {
                // Combine the attributes into a single list, by moving the elements of 2 into 1.
                attributes1.extend(attributes2);
                (attributes1, comment)
            },
        ))
    }

    fn file_attributes(input: PestNode) -> PestResult<Vec<Attribute>> {
        Ok(match_nodes!(input.into_children();
            [attribute(attributes)..] => attributes.collect(),
        ))
    }

    fn local_attributes(input: PestNode) -> PestResult<Vec<Attribute>> {
        Ok(match_nodes!(input.into_children();
            [attribute(attributes)..] => attributes.collect(),
        ))
    }

    fn attribute(input: PestNode) -> PestResult<Attribute> {
        let location = from_span(&input);

        Ok(match_nodes!(input.into_children();
            [attribute_directive(attribute)] => {
                let (prefix, directive) = attribute;
                Attribute::new(prefix, directive, Vec::new(), location)
            },
            [attribute_directive(attribute), attribute_arguments(arguments)] => {
                let (prefix, directive) = attribute;
                Attribute::new(prefix, directive, arguments, location)
            },
        ))
    }

    fn attribute_directive(input: PestNode) -> PestResult<(Option<String>, String)> {
        Ok(match_nodes!(input.into_children();
            [attribute_identifier(name)] => (None, name),
            [attribute_identifier(prefix), attribute_identifier(name)] => (Some(prefix), name)
        ))
    }

    fn attribute_identifier(input: PestNode) -> PestResult<String> {
        Ok(input.as_str().to_owned())
    }

    fn attribute_argument(input: PestNode) -> PestResult<String> {
        let argument = input.as_str();
        // If the argument was wrapped in quotes, remove them.
        if argument.starts_with('"') && argument.ends_with('"') {
            let mut chars = argument.chars();
            // Skip the first and last characters (they're just quotes).
            chars.next();
            chars.next_back();
            Ok(chars.collect::<String>())
        } else {
            Ok(argument.to_owned())
        }
    }

    fn attribute_arguments(input: PestNode) -> PestResult<Vec<String>> {
        Ok(match_nodes!(input.into_children();
            [attribute_argument(argument)] => {
                vec![argument]
            },
            [attribute_argument(argument), attribute_arguments(mut list)] => {
                // The argument comes before the rest of the arguments when parsing, so we have to
                // insert the new argument at the front of the list.
                list.insert(0, argument);
                list
            },
        ))
    }

    fn doc_comment(input: PestNode) -> PestResult<Option<DocComment>> {
        let location = from_span(&input);
        Ok(match_nodes!(input.into_children();
            [] => {
                None
            },
            [line_doc_comment(comments)..] => {
                // Merge all the line comments together.
                let combined = comments.collect::<Vec<String>>().join("\n");
                Some(CommentParser::parse_doc_comment(&combined, location))
            },
            [block_doc_comment(comment)] => {
                Some(CommentParser::parse_doc_comment(&comment, location))
            }
        ))
    }

    fn line_doc_comment(input: PestNode) -> PestResult<String> {
        Ok(input.as_str().to_owned())
    }

    fn block_doc_comment(input: PestNode) -> PestResult<String> {
        Ok(input.as_str().to_owned())
    }

    fn integer(input: PestNode) -> PestResult<i64> {
        let int = input.as_str().parse::<i64>();
        match int {
            Ok(int) => Ok(int),
            Err(err) => Err(PestError::new_from_span(
                PestErrorVariant::CustomError {
                    message: format!("Failed to parse integer: {}", err),
                },
                input.as_span(),
            )),
        }
    }

    fn compact_id(input: PestNode) -> PestResult<Option<u32>> {
        Ok(match_nodes!(input.into_children();
            []               => None,
            [integer(value)] => {
                // compact ids must fit in an i32 and be non-negative.
                if value < 0 || value > i32::MAX.into() {
                    // TODO let location = from_span(&input);
                    // TODO let error_string = if integer < 0 {
                    // TODO     format!("ID is out of range: {}. Compact IDs must be positive", integer)
                    // TODO } else {
                    // TODO     format!(
                    // TODO         "ID is out of range: {}. Compact IDs must be less than {}",
                    // TODO         integer, i32::MAX
                    // TODO     )
                    // TODO };
                    // TODO report an error here!
                }
                Some(value as u32)
            }
        ))
    }

    fn stream_modifier(input: PestNode) -> PestResult<bool> {
        Ok(match_nodes!(input.into_children();
            []                => false,
            [stream_kw(_)] => true
        ))
    }

    fn idempotent_modifier(input: PestNode) -> PestResult<bool> {
        Ok(match_nodes!(input.into_children();
            []                => false,
            [idempotent_kw(_)] => true
        ))
    }

    fn unchecked_modifier(input: PestNode) -> PestResult<bool> {
        Ok(match_nodes!(input.into_children();
            []                => false,
            [unchecked_kw(_)] => true
        ))
    }

    fn module_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn struct_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn class_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn exception_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn interface_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn enum_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn type_alias_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn sequence_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn dictionary_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn void_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn bool_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn byte_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn short_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn ushort_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn int_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn uint_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn varint_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn varuint_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn long_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn ulong_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn varlong_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn varulong_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn float_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn double_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn string_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn tag_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn stream_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn extends_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn idempotent_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn unchecked_kw(input: PestNode) -> PestResult<()> {
        Ok(())
    }

    fn EOI(input: PestNode) -> PestResult<()> {
        Ok(())
    }
}
