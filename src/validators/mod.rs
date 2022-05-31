// Copyright (c) ZeroC, Inc. All rights reserved.

mod attribute;
mod dictionary;
mod enums;
mod identifiers;
mod tag;

use crate::ast::Ast;
use crate::error::ErrorReporter;
use crate::grammar::*;
use crate::slice_file::SliceFile;
use crate::validators::{
    AttributeValidator, DictionaryValidator, EnumValidator, IdentifierValidator, TagValidator,
};
use crate::visitor::Visitor;
use std::collections::HashMap;

// Re-export the contents of the validators submodules directly into the validators module. This is
// for convenience, so users don't need to worry about the submodule structure while importing.
pub use self::attribute::*;
pub use self::dictionary::*;
pub use self::enums::*;
pub use self::identifiers::*;
pub use self::tag::*;

pub(crate) struct Validator<'a> {
    pub error_reporter: &'a mut ErrorReporter,
    pub ast: &'a Ast,
}

impl Validator<'_> {
    /// This method is responsible for visiting each slice file with the various validators.
    pub fn validate(&mut self, slice_files: &HashMap<String, SliceFile>) {
        for slice_file in slice_files.values() {
            slice_file.visit_with(self);
            slice_file.visit_with(&mut AttributeValidator { error_reporter: self.error_reporter });
            slice_file.visit_with(&mut EnumValidator { error_reporter: self.error_reporter });
            slice_file.visit_with(&mut TagValidator { error_reporter: self.error_reporter });
            slice_file.visit_with(&mut IdentifierValidator { error_reporter: self.error_reporter });

            let dictionary_validator =
                &mut DictionaryValidator { error_reporter: self.error_reporter, ast: self.ast };
            slice_file.visit_with(dictionary_validator);
            dictionary_validator.validate_dictionary_key_types();
        }
    }

    fn validate_stream_member(&mut self, members: Vec<&Parameter>) {
        // If members is empty, `split_last` returns None, and this check is skipped,
        // otherwise it returns all the members, except for the last one. None of these members
        // can be streamed, since only the last member can be.
        if let Some((_, nonstreamed_members)) = members.split_last() {
            for member in nonstreamed_members {
                if member.is_streamed {
                    self.error_reporter.report_error(
                        "only the last parameter in an operation can use the stream modifier",
                        Some(&member.location),
                    );
                }
            }
        }
    }
}

impl<'a> Visitor for Validator<'a> {
    fn visit_struct_start(&mut self, struct_def: &Struct) {
        if struct_def.is_compact {
            // Compact structs must be non-empty.
            if struct_def.members().is_empty() {
                self.error_reporter.report_error(
                    "compact structs must be non-empty",
                    Some(&struct_def.location),
                )
            }
        }
    }

    fn visit_operation_start(&mut self, operation_def: &Operation) {
        self.validate_stream_member(operation_def.all_members());
    }
}
