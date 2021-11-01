// Copyright (c) ZeroC, Inc. All rights reserved.

use crate::grammar::{Attribute, Module};
use crate::ptr_util::WeakPtr;

#[derive(Clone, Debug)]
pub struct Location {
    pub start: (usize, usize),
    pub end: (usize, usize),
    pub file: String,
}

pub struct SliceFile {
    pub filename: String,
    pub relative_path: String,
    pub raw_text: String,
    pub contents: Vec<WeakPtr<Module>>,
    pub attributes: Vec<Attribute>,
    pub is_source: bool,
    line_positions: Vec<usize>,
}

impl SliceFile {
    pub fn new(
        relative_path: String,
        raw_text: String,
        contents: Vec<WeakPtr<Module>>,
        attributes: Vec<Attribute>,
        is_source: bool
    ) -> SliceFile {
        // Store the starting position of each line the file.
        // Slice supports '\n', '\r', and '\r\n' as newlines.
        let mut line_positions = vec![0]; // The first line always starts at index 0.
        let mut last_char_was_carriage_return = false;

        // Iterate through each character in the file.
        // If we hit a '\n' we immediately store `index + 1` as the starting position for the next
        // line (`+ 1` because the line starts after the newline character).
        // If we hit a '\r' we wait and read the next character to see if it's a '\n'.
        // If so, the '\n' block handles it, otherwise we store `index`
        // (no plus one, because we've already read ahead to the next character).
        for (index, character) in raw_text.chars().enumerate() {
            if character == '\n' {
                line_positions.push(index + 1);
                last_char_was_carriage_return = false;
            } else {
                if last_char_was_carriage_return {
                    line_positions.push(index);
                }
                last_char_was_carriage_return = character == '\r';
            }
        }

        // Extract the name of the slice file without its extension.
        let filename = std::path::Path::new(&relative_path)
            .file_stem()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap();

        SliceFile {
            filename,
            relative_path,
            raw_text,
            contents,
            attributes,
            is_source,
            line_positions,
        }
    }
}
