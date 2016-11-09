// Copyright (c) 2016 Fabian Schuiki
extern crate owning_ref;
extern crate clap;
pub mod lexer;
pub mod svlog;
pub mod vhdl;
pub mod errors;
pub mod name;
pub mod source;
use clap::{Arg, App};


fn main() {
	let matches = App::new("moore")
		.arg(Arg::with_name("inc")
			.short("I")
			.value_name("DIR")
			.help("Adds a search path for SystemVerilog includes")
			.multiple(true)
			.takes_value(true)
			.number_of_values(1))
		.arg(Arg::with_name("INPUT")
			.help("The input file to use")
			.required(true)
			.index(1))
		.get_matches();

	// Prepare a list of include paths.
	let include_paths: Vec<_> = match matches.values_of("inc") {
		Some(args) => args.map(|x| std::path::Path::new(x)).collect(),
		None => Vec::new()
	};
	let filename = matches.value_of("INPUT").unwrap();

	// Run the input file through the SystemVerilog preprocessor for now.
	let sm = source::get_source_manager();
	let pp = svlog::preproc::Preprocessor::new(sm.open(&filename).unwrap(), &include_paths);
	for res in pp {
		print!("{}", res.unwrap().1.extract());
	}
}
