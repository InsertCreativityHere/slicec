
mod cs_writer;
mod cs_options;
mod cs_util;
mod cs_validator;

use cs_options::CsOptions;
use cs_validator::CsValidator;
use cs_writer::CsWriter;
use structopt::StructOpt;

pub fn main() {
    match try_main() {
        Ok(()) => { println!("SUCCESS"); },
        Err(()) => { println!("FAILED"); },
    };
}

fn try_main() -> Result<(), ()> {
    let options = CsOptions::from_args();
    let mut data = slice::parse_from_options(&options.slice_options)?;

    let mut cs_validator = CsValidator::new(&mut data.error_handler);
    for slice_file in data.slice_files.values() {
        slice_file.visit_with(&mut cs_validator, &data.ast);
    }
    slice::handle_errors(options.slice_options.warn_as_error, &mut data.error_handler, &data.slice_files)?;

    for slice_file in data.slice_files.values() {
        let mut writer = CsWriter::new(&slice_file.filename).unwrap();
        slice_file.visit_with(&mut writer, &data.ast);
        writer.close();
    }

    Ok(())
}
