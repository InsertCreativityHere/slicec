// Copyright (c) ZeroC, Inc. All rights reserved.

use crate::errors::*;
use crate::grammar::*;
use crate::validators::{ValidationChain, Validator};

pub fn identifier_validators() -> ValidationChain {
    vec![
        Validator::Identifiers(check_for_redefinition),
        Validator::InheritedIdentifiers(check_for_shadowing),
    ]
}

pub fn check_for_redefinition(mut identifiers: Vec<&Identifier>, error_reporter: &mut ErrorReporter) {
    // Sort first so that we can use windows to search for duplicates.
    identifiers.sort_by_key(|identifier| identifier.value.to_owned());
    identifiers.windows(2).for_each(|window| {
        if window[0].value == window[1].value {
            let error = LogicKind::Redefinition(window[1].value.clone());
            error_reporter.report(error, Some(window[1].span()));
            error_reporter.report(
                ErrorKind::new_note(format!("`{}` was previously defined here", window[0].value)),
                Some(window[0].span()),
            );
        }
    });
}

pub fn check_for_shadowing(
    identifiers: Vec<&Identifier>,
    inherited_symbols: Vec<&Identifier>,
    error_reporter: &mut ErrorReporter,
) {
    identifiers.iter().for_each(|identifier| {
        inherited_symbols
            .iter()
            .filter(|inherited_identifier| inherited_identifier.value == identifier.value)
            .for_each(|inherited_identifier| {
                let error = LogicKind::Shadows(identifier.value.clone());
                error_reporter.report(error, Some(identifier.span()));
                error_reporter.report(
                    ErrorKind::new_note(format!("`{}` was previously defined here", inherited_identifier.value)),
                    Some(inherited_identifier.span()),
                );
            });
    });
}

trait EntityIdentifiersExtension {
    fn get_identifiers(&self) -> Vec<&Identifier>;
}

impl<T> EntityIdentifiersExtension for Vec<&T>
where
    T: Entity,
{
    fn get_identifiers(&self) -> Vec<&Identifier> {
        self.iter().map(|member| member.raw_identifier()).collect()
    }
}
