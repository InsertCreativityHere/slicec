// Copyright (c) ZeroC, Inc. All rights reserved.

use super::super::*;
use crate::ptr_util::WeakPtr;
use crate::slice_file::Location;
use crate::supported_encodings::SupportedEncodings;

#[derive(Debug)]
pub struct TypeRef<T: Element + ?Sized = dyn Type> {
    pub type_string: String,
    pub definition: WeakPtr<T>,
    pub is_optional: bool,
    pub scope: Scope,
    pub attributes: Vec<Attribute>,
    pub location: Location,
}

impl<T: Element + ?Sized + 'static> TypeRef<T> {
    pub(crate) fn new(
        type_string: String,
        is_optional: bool,
        scope: Scope,
        attributes: Vec<Attribute>,
        location: Location,
    ) -> Self {
        let definition = WeakPtr::create_uninitialized();
        TypeRef { type_string, definition, is_optional, scope, attributes, location }
    }
}

impl<T: Element + ?Sized> TypeRef<T> {
    pub fn definition(&self) -> &T {
        self.definition.borrow()
    }

    pub(crate) fn downcast<U: Element + 'static>(&self) -> Result<TypeRef<U>, ()> {
        let definition = if self.definition.is_initialized() {
            match self.definition.clone().downcast::<U>() {
                Ok(ptr) => ptr,
                Err(_) => return Err(()),
            }
        } else {
            WeakPtr::create_uninitialized()
        };

        Ok(TypeRef {
            type_string: self.type_string.clone(),
            definition,
            is_optional: self.is_optional,
            scope: self.scope.clone(),
            attributes: self.attributes.clone(),
            location: self.location.clone(),
        })
    }
}

impl<T: Type + ?Sized> TypeRef<T> {
    pub fn is_bit_sequence_encodable(&self) -> bool {
        self.is_optional && self.min_wire_size() == 0
    }

    // This intentionally shadows the trait method of the same name on `Type`.
    pub fn is_fixed_size(&self) -> bool {
        !self.is_optional && T::is_fixed_size(self)
    }

    // This intentionally shadows the trait method of the same name on `Type`.
    pub fn min_wire_size(&self) -> u32 {
        if self.is_optional {
            match self.definition().concrete_type() {
                // TODO explain why still take up 1 byte.
                // TODO this is not totally correct the min_wire_size of a optional interface
                // depends on the encoding
                Types::Class(_) => 1,
                Types::Primitive(primitive) if matches!(primitive, Primitive::AnyClass) => 1,
                _ => 0,
            }
        } else {
            T::min_wire_size(self)
        }
    }

    // This intentionally shadows the trait method of the same name on `Type`.
    pub fn supported_encodings(&self) -> SupportedEncodings {
        let mut supported_encodings = self.definition().supported_encodings();
        if self.is_optional {
            // Optional data types are not supported with the Slice1 encoding.
            // Note that this doesn't include tagged data members and parameters, which are allowed.
            // Even though they're marked with a '?' these are not technically optional types.
            supported_encodings.disable(Encoding::Slice1);
        }
        supported_encodings
    }
}

impl<T: Element + ?Sized> Clone for TypeRef<T> {
    fn clone(&self) -> Self {
        TypeRef {
            type_string: self.type_string.clone(),
            definition: self.definition.clone(),
            is_optional: self.is_optional,
            scope: self.scope.clone(),
            attributes: self.attributes.clone(),
            location: self.location.clone(),
        }
    }
}

impl<T: Element + ?Sized> Attributable for TypeRef<T> {
    fn attributes(&self) -> &Vec<Attribute> {
        &self.attributes
    }

    fn get_raw_attribute(&self, directive: &str, recurse: bool) -> Option<&Attribute> {
        if recurse {
            panic!("Cannot recursively get attributes on a typeref");
        }

        for attribute in &self.attributes {
            if attribute.prefixed_directive == directive {
                return Some(attribute);
            }
        }
        None
    }
}

impl<T: Element + ?Sized> std::ops::Deref for TypeRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.definition()
    }
}

implement_Element_for!(TypeRef<T>, "type reference", Element + ?Sized);
implement_Symbol_for!(TypeRef<T>, Element + ?Sized);
implement_Scoped_Symbol_for!(TypeRef<T>, Element + ?Sized);
