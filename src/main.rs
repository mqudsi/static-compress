#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate stderr;
extern crate chan;
extern crate clap;
extern crate filetime;
extern crate glob;

#[macro_use]
mod errors;
mod structs;
mod compression;

use clap::{App, Arg};
use errors::*;
use structs::*;
use std::sync::Arc;

quick_main!(run);

fn run() -> Result<()> {
    let matches = App::new("static-compress")
        .version("0.1")
        .about("Create statically-compresed copies of matching files")
        .author("NeoSmart Technologies")
        .arg(Arg::with_name("compressor")
            .short("c")
            .long("compressor")
            .value_name("[brotli|gzip|zopfli]")
            .help("The compressor to use")
            .takes_value(true))
        .arg(Arg::with_name("threads")
            .short("j")
            .long("threads")
            .value_name("COUNT")
            .help("The number of simultaneous conversions")
            .takes_value(true))
        .arg(Arg::with_name("filters")
            .value_name("FILTER")
            .multiple(true)
            .required(true))
        .arg(Arg::with_name("ext")
            .short("e")
            .value_name(".EXT")
            .long("extension")
            .help("The extension to use for compressed files. Supplied automatically if not provided."))
        .get_matches();

    fn get_parameter<'a, T>(matches: &clap::ArgMatches, name: &str, default_value: T) -> Result<T>
        where T: std::str::FromStr
    {
        match matches.value_of(name) {
            Some(v) => {
                Ok(v.parse().map_err(|_| ErrorKind::InvalidParameterValue(name.to_owned()))?)
            }
            None => Ok(default_value),
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
            None => Err(ErrorKind::InvalidUsage),
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
                Ok(path) => {
                    //make sure this is a file, not a folder
                    match std::fs::metadata(&path) {
                        Ok(metadata) => {
                            if metadata.is_file() {
                                send_queue.send(path);
                            }
                            continue; //skip otherwise
                        }
                        Err(e) => errstln!("{}: {}", path.to_string_lossy(), e),
                    }
                }
                Err(e) => errstln!("{}", e),
            };
        }
    }
    Ok(())
}

fn worker_thread(params: Arc<Parameters>, rx: chan::Receiver<ThreadParam>) {
    loop {
        let src = match rx.recv() {
            Some(task) => task,
            None => return, //no more tasks
        };

        //in a nested function so we can handle errors centrally
        fn compress_single(src: &ThreadParam, params: &Parameters) -> Result<()> {
            let dst_path = format!("{}.{}",
                                   src.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?,
                                   params.extension);
            let dst = std::path::Path::new(&dst_path);

            //again, in a scope for error handling
            || -> Result<()> {
                    println!("{}", src.to_string_lossy());
                    let src_metadata = std::fs::metadata(src)?;

                    //don't compress files that are already compressed that haven't changed
                    if let Ok(dst_metadata) = std::fs::metadata(dst) {
                        //the destination already exists
                        match src_metadata.modified()? == dst_metadata.modified()? {
                            true => return Ok(()), //no need to recompress
                            false => std::fs::remove_file(dst)?, //throw if we can't
                        };
                    }
                    params.compressor.compress(src.as_path(), dst)?;
                    let src_modified = filetime::FileTime::from_last_modification_time(&src_metadata);
                    filetime::set_file_times(dst, filetime::FileTime::zero(), src_modified).unwrap_or_default();

                    Ok(())
                }()
                .map_err(|e| {
                    //try deleting the invalid destination file, but don't care if we can't
                    std::fs::remove_file(dst).unwrap_or_default();
                    e //return the same error
                })
        }

        if let Err(e) = compress_single(&src, &params) {
            errstln!("Error compressing {}: {}", src.to_string_lossy(), e);
        }
    }
}
