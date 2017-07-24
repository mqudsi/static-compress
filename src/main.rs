#[macro_use] extern crate error_chain;
#[macro_use] extern crate prettytable;
#[macro_use] extern crate stderr;
extern crate chan;
extern crate clap;
extern crate filetime;
extern crate globset;
extern crate pretty_bytes;
extern crate separator;

#[macro_use] mod errors;
mod compression;
mod lists;
mod structs;

use clap::{App, Arg};
use errors::*;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use lists::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use structs::*;

const DEBUG_FILTERS: bool = cfg!(debug_assertions);
fn debug(message: &str) {
    if DEBUG_FILTERS {
        errstln!("{}", message);
    }
}

quick_main!(run);

fn run() -> Result<()> {
    let matches = App::new("static-compress")
        .version("0.3")
        .about("Create statically-compresed copies of matching files")
        .author("NeoSmart Technologies")
        .arg(Arg::with_name("compressor")
            .short("c")
            .long("compressor")
            .value_name("[brotli|gzip|zopfli|webp]")
            .help("The compressor to use (default: gzip)")
            .takes_value(true))
        .arg(Arg::with_name("threads")
            .short("j")
            .long("threads")
            .value_name("COUNT")
            .help("The number of simultaneous compressions (default: 1)")
            .takes_value(true))
        .arg(Arg::with_name("filters")
            .value_name("FILTER")
            .multiple(true)
            .required(true))
        .arg(Arg::with_name("ext")
            .short("e")
            .value_name("EXT")
            .long("extension")
            .help("The extension to use for compressed files (default: gz, br, or webp)"))
        /*.arg(Arg::with_name("excludes")
            .short("x")
            .value_name("FILTER")
            .long("exclude")
            .multiple(true)
            .help("Exclude files matching this glob expression"))*/
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

    let case_sensitive = !matches.is_present("nocase");
    let compressor = get_parameter(&matches, "compressor", CompressionAlgorithm::GZip)?;
    let temp = Parameters {
        extension: matches.value_of("ext")
            .unwrap_or(compressor.extension())
            .trim_matches(|c: char| c.is_whitespace() || c.is_control() || c == '.')
            .to_owned(),
        compressor: compressor,
        threads: get_parameter(&matches, "threads", 1)?,
    };

    /*let exclude_filters = match matches.values_of("exclude") {
        Some(values)=> values.map(|s| s.to_owned()).collect(),
        None => Vec::<String>::new(),
    };*/

    let parameters = Arc::<Parameters>::new(temp);
    let (send_queue, stats_rx, wait_group) = start_workers(&parameters);

    let include_filters: Vec<String> = match matches.values_of("filters") {
        Some(values) => Ok(values.map(|s| s.to_owned()).collect()),
        None => Err(ErrorKind::InvalidUsage),
    }?;

    let mut builder = GlobSetBuilder::new();
    for filter in include_filters.iter() {
        let glob = GlobBuilder::new(filter)
            .case_insensitive(!case_sensitive)
            .literal_separator(true)
            .build().map_err(|_| ErrorKind::InvalidIncludeFilter)?;
        builder.add(glob);
    }
    let globset = builder.build().map_err(|_| ErrorKind::InvalidIncludeFilter)?;

    //convert filters to paths and deal out conversion jobs
    dispatch_jobs(send_queue, include_filters, globset/*, exclude_filters*/)?;

    //wait for all jobs to finish
    wait_group.wait();

    //merge statistics from all threads
    let mut stats = Statistics::new();
    while let Ok(thread_stats) = stats_rx.recv() {
        stats.merge(&thread_stats);
    }

    println!("{}", stats);

    Ok(())
}

type ThreadParam = std::path::PathBuf;

fn start_workers<'a>(params: &Arc<Parameters>) -> (chan::Sender<ThreadParam>, mpsc::Receiver<Statistics>, chan::WaitGroup) {
    let (tx, rx) = chan::sync::<ThreadParam>(params.threads);
    let (stats_tx, stats_rx) = std::sync::mpsc::channel::<Statistics>();
    let wg = chan::WaitGroup::new();

    for _ in 0..params.threads {
        let local_params = params.clone();
        let local_rx = rx.clone();
        let local_stats_tx = stats_tx.clone();
        let local_wg = wg.clone();
        wg.add(1);
        std::thread::spawn(move || {
            worker_thread(local_params, local_stats_tx, local_rx);
            local_wg.done();
        });
    }

    (tx, stats_rx, wg)
}

fn yield_file<F>(path: PathBuf, globset: &GlobSet, callback: &F) -> Result<()>
    where F: Fn(PathBuf) -> Result<()>
{
    if is_hidden(&path)? {
        //we are ignoring .files and .directories
        //we may add a command-line switch to control this behavior in the future
        return Ok(());
    }

    if path.is_dir() {
        for child in path.read_dir()? {
            let child_path = child?.path();
            yield_file(child_path, globset, callback)?;
        }
    }
    else {
        //I'm presuming the binary search in is_blacklisted is faster
        //than globset.is_match, but we should benchmark it at some point
        //debug(&format!("{}: is_blacklisted: {}, globset.is_match: {}", path.display(), is_blacklisted(&path)?, globset.is_match(&path)));
        if !is_blacklisted(&path)? && globset.is_match(&path) {
            callback(path)?;
        }
    }

    Ok(())
}

fn dispatch_jobs(send_queue: chan::Sender<ThreadParam>, filters: Vec<String>, globset: GlobSet/*, exclude_filters: Vec<String>*/) -> Result<()> {
    let paths = extract_paths(&filters)?;
    for path in paths {
        yield_file(path, &globset, &|path: PathBuf| {
            send_queue.send(path);
            Ok(())
        })?
    }

    Ok(())
}

fn worker_thread(params: Arc<Parameters>, stats_tx: mpsc::Sender<Statistics>, rx: chan::Receiver<ThreadParam>) {
    let mut local_stats = Statistics::new();

    loop {
        let src = match rx.recv() {
            Some(task) => task,
            None => break, //no more tasks
        };

        //in a nested function so we can handle errors centrally
        fn compress_single(src: &ThreadParam, params: &Parameters, mut local_stats: &mut Statistics) -> Result<()> {
            let dst_path = format!("{}.{}",
                                   src.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?,
                                   params.extension);
            let dst = Path::new(&dst_path);

            //again, in a scope for error handling
            |local_stats: &mut Statistics| -> Result<()> {
                    println!("{}", src.to_string_lossy());
                    let src_metadata = std::fs::metadata(src)?;

                    //don't compress files that are already compressed that haven't changed
                    if let Ok(dst_metadata) = std::fs::metadata(dst) {
                        //the destination already exists
                        let src_seconds = src_metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
                        let dst_seconds = dst_metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
                        match src_seconds == dst_seconds {
                            true => {
                                local_stats.update(src_metadata.len(), dst_metadata.len(), false);
                                return Ok(());//no need to recompress
                            },
                            false => {
                                std::fs::remove_file(dst)?; //throw if we can't
                            }
                        };
                    }
                    params.compressor.compress(src.as_path(), dst)?;
                    let dst_metadata = std::fs::metadata(dst)?;
                    local_stats.update(src_metadata.len(), dst_metadata.len(), true);
                    let src_modified = filetime::FileTime::from_last_modification_time(&src_metadata);
                    filetime::set_file_times(dst, filetime::FileTime::zero(), src_modified).unwrap_or_default();

                    Ok(())
                }(&mut local_stats)
                .map_err(|e| {
                    //try deleting the invalid destination file, but don't care if we can't
                    std::fs::remove_file(dst).unwrap_or_default();
                    e //return the same error
                })
        }

        if let Err(e) = compress_single(&src, &params, &mut local_stats) {
            errstln!("Error compressing {}: {}", src.to_string_lossy(), e);
        }
    }

    if !stats_tx.send(local_stats).is_ok() {
        errstln!("Error compiling statistics!");
    }
}

fn str_search(sorted: &[&str], search_term: &str, case_sensitive: bool) -> std::result::Result<usize, usize> {
    let term = match case_sensitive {
        true => search_term.to_owned(),
        false => search_term.to_lowercase(),
    };

    sorted.binary_search_by(|probe| probe.cmp(&&*term))
}

fn is_hidden(path: &Path) -> Result<bool> {
    let hidden = match path.file_name() {
        Some(x) => x.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?
            .starts_with("."),
        None => false
    };
    Ok(hidden)
}

fn is_blacklisted(path: &Path) -> Result<bool> {
    let r = match path.extension() {
        Some(x) => {
            let ext = x.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?;
            str_search(COMP_EXTS, &ext, false).is_ok()
        },
        None => false,
    };

    return Ok(r);
}

//Given a list of filters, extracts the directories that should be searched
//To-Do: Also provide info about to what depth they should be recursed
use std::collections::HashSet;
fn extract_paths(filters: &Vec<String>) -> Result<HashSet<PathBuf>> {
    use std::iter::FromIterator;

    let mut dirs = std::collections::HashSet::<PathBuf>::new();

    {
        let insert_path = &mut |filter: &String, dir: PathBuf| {
            debug(&format!("filter {} mapped to search {}", filter, dir.display()));
            dirs.insert(dir);
        };

        for filter in filters {
            //take everything until the first expression
            let mut last_char = None::<char>;
            let dir;
            {
                let partial = filter.chars().take_while(|c| match c {
                    &'?' | &'*' | &'{' | &'[' => false,
                    c => { last_char = Some(c.clone()); true }
                });
                dir = String::from_iter(partial);
            }

            let dir = match dir.chars().next() {
                Some(c) => match c {
                    '.' | '/' => PathBuf::from(dir),
                    _ => {
                        let mut pb = PathBuf::from("");
                        pb.push(dir);
                        pb
                    }
                },
                None => {
                    insert_path(filter, PathBuf::from(""));
                    continue;
                }
            };

            if dir.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?.ends_with(filter) {
                //the "dir" is actually a full path to a single file
                //return it as-is
                insert_path(filter, dir);
                continue;
            }

            if last_char == Some('/') {
                //dir is a already a directory, return it as-is
                insert_path(filter, dir);
                continue;
            }

            //we need to extract the directory from the path we have
            let dir = match PathBuf::from(dir).parent() {
                Some(parent) => parent.to_path_buf(),
                None => PathBuf::from(""),
            };

            insert_path(filter, dir);
        }
    }

    debug(&format!("final search paths: {:?}", dirs));

    Ok(dirs)
}
