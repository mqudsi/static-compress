#[macro_use]
extern crate error_chain;
extern crate clap;

#[macro_use]
mod errors;
mod structs;

use clap::{App, Arg};
use errors::*;
use structs::*;

quick_main!(run);

fn run() -> Result<()> {
    let matches =
        App::new("static-compress")
        .version("0.1")
        .about("Create statically-compresed copies of matching files")
        .author("NeoSmart Technologies")
        .arg(
            Arg::with_name("compressor")
            .short("c")
            .long("compressor")
            .value_name("[brotli|gzip|zopfli]")
            .help("The compressor to use")
            .takes_value(true)
            )
        .arg(
            Arg::with_name("threads")
            .short("j")
            .long("threads")
            .value_name("COUNT")
            .help("The number of simultaneous conversions")
            .takes_value(true)
            )
        .arg(
            Arg::with_name("filters")
            .value_name("FILTER")
            .multiple(true)
            .required(true)
            )
        .get_matches();

    fn get_parameter<'a, T>(matches: &clap::ArgMatches, name: &str, default_value: T) -> Result<T>
        where T: std::str::FromStr,
    {
        match matches.value_of(name) {
            Some(v) => Ok(v.parse().map_err(|_| ErrorKind::InvalidParameterValue(name.to_owned()))?),
            None => Ok(default_value)
        }
    }

    let parameters = Parameters {
        compressor: get_parameter(&matches, "compressor", CompressionAlgorithm::GZip)?,
        include_filters: match matches.values_of("filters") {
            Some(values) => Ok(values.map(|s| s.to_owned()).collect()),
            None => Err(ErrorKind::InvalidUsage)
        }?,
        threads: get_parameter(&matches, "threads", 1)?,
    };

    Ok(())
}
