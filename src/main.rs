#[macro_use] extern crate error_chain;
#[macro_use] extern crate stderr;
extern crate chan;
extern crate clap;
extern crate glob;

#[macro_use]
mod errors;
mod structs;
mod compression;

use clap::{App, Arg};
use errors::*;
use structs::*;
use std::sync::Arc;
use compression::*;

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

    let compressor = get_parameter(&matches, "compressor", CompressionAlgorithm::GZip)?;
    let temp = Parameters {
        extension: matches.value_of("ext")
            .unwrap_or(compressor.extension())
            .trim_matches(|c: char| c.is_whitespace() || c.is_control() || c == '.')
            .to_owned(),
        compressor: compressor,
        include_filters: match matches.values_of("filters") {
            Some(values) => Ok(values.map(|s| s.to_owned()).collect()),
            None => Err(ErrorKind::InvalidUsage)
        }?,
        threads: get_parameter(&matches, "threads", 1)?,
    };

    let parameters = Arc::<Parameters>::new(temp);
    let (send_queue, wait_group) = start_workers(&parameters);

    //convert filters to paths and deal out conversion jobs
    dispatch_jobs(send_queue, &parameters.include_filters)?;

    //wait for all jobs to finish
    wait_group.wait();

    Ok(())
}

type ThreadParam = std::path::PathBuf;

fn start_workers<'a>(params: &Arc<Parameters>) -> (chan::Sender<ThreadParam>, chan::WaitGroup) {
    let (tx, rx) = chan::sync::<ThreadParam>(params.threads);
    let wg = chan::WaitGroup::new();

    for _ in 0..params.threads {
        let local_params = params.clone();
        let local_rx = rx.clone();
        let local_wg = wg.clone();
        wg.add(1);
        std::thread::spawn(move || {
            worker_thread(local_params, local_rx);
            local_wg.done();
        });
    }

    (tx, wg)
}

fn dispatch_jobs(send_queue: chan::Sender<ThreadParam>, filters: &Vec<String>) -> Result<()> {
    for filter in filters {
        for entry in glob::glob(filter).map_err(|_| ErrorKind::InvalidIncludeFilter)? {
            match entry {
                Ok(path) => send_queue.send(path),
                Err(e) => errstln!("{:?}", e) //error reading file, but don't bail
            }
        };
    }
    Ok(())
}

fn worker_thread(params: Arc<Parameters>, rx: chan::Receiver<ThreadParam>) {
    loop {
        let src = match rx.recv() {
            Some(task) => task,
            None => return //no more tasks
        };

        let dst_path = format!("{}.{}", src.to_str().unwrap(), params.extension);
        let dst = std::path::Path::new(&dst_path);

        println!("{}", src.to_string_lossy());

        //params.compressor.compress(src, dst, params.level);
        if let Err(e) = params.compressor.compress(src.as_path(), dst) {
            errstln!("Error compressing {}: {}", src.to_string_lossy(), e);
        }
    }
}

