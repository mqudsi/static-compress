#[macro_use] extern crate error_chain;
#[macro_use] extern crate prettytable;
#[macro_use] extern crate stderr;
extern crate chan;
extern crate clap;
extern crate filetime;
extern crate globset;
extern crate separator;
extern crate size;

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
        .version("0.3.3")
        .about("Create statically-compresed copies of matching files")
        .author("Mahmoud Al-Qudsi, NeoSmart Technologies")
        .arg(Arg::new("compressor")
            .short('c')
            .long("compressor")
            .value_name("[brotli|gzip|zopfli|webp]")
            .help("The compressor to use (default: gzip)")
            .takes_value(true))
        .arg(Arg::new("threads")
            .short('j')
            .long("threads")
            .value_name("COUNT")
            .help("The number of simultaneous compressions (default: 1)")
            .takes_value(true))
        .arg(Arg::new("filters")
            .value_name("FILTER")
            .multiple_occurrences(true)
            .required(true))
        .arg(Arg::new("ext")
            .short('e')
            .value_name("EXT")
            .long("extension")
            .help("The extension to use for compressed files (default: gz, br, or webp)"))
        .arg(Arg::new("quality")
             .short('q')
             .long("quality")
             .takes_value(true)
             .help("A quality parameter to be passed to the encoder. Algorithm-specific."))
        .arg(Arg::new("quiet")
             .long("quiet")
             .takes_value(false)
             .help("Does not display progress or end-of-run summary table."))
        .arg(Arg::new("no-progress")
             .long("no-progress")
             .takes_value(false)
             .help("Do not list files as they are compressed."))
        .arg(Arg::new("no-summary")
             .long("no-summary")
             .takes_value(false)
             .help("Hide end-of-run statistics summary."))
        .arg(Arg::new("nocase")
             .short('i')
             .long("case-insensitive")
             .takes_value(false)
             .help("Use case-insensitive pattern matching."))
        /*.arg(Arg::new("excludes")
            .short('x')
            .value_name("FILTER")
            .long("exclude")
            .multiple(true)
            .help("Exclude files matching this glob expression"))*/
        .get_matches();

    fn get_parameter<'a, T>(matches: &clap::ArgMatches, name: &'static str, default_value: T) -> Result<T>
        where T: std::str::FromStr
    {
        match matches.value_of(name) {
            Some(v) => {
                Ok(v.parse().map_err(|_| ErrorKind::InvalidParameterValue(name))?)
            }
            None => Ok(default_value),
        }
    }

    let case_sensitive = !matches.is_present("nocase");
    let compressor = get_parameter(&matches, "compressor", CompressionAlgorithm::GZip)?;
    let show_summary = !matches.contains_id("no-summary") && !matches.contains_id("quiet");
    let show_progress = !matches.contains_id("no-progress") && !matches.contains_id("quiet");

    let parameters = Arc::new(Parameters {
        extension: matches.value_of("ext")
            .unwrap_or(compressor.extension())
            .trim_matches(|c: char| c.is_whitespace() || c.is_control() || c == '.')
            .to_owned(),
        compressor,
        quality: match matches.value_of("quality") {
            Some(q) => Some(q.parse::<u8>().map_err(|_| ErrorKind::InvalidParameterValue("quality"))?),
            None => None
        },
        show_summary,
        show_progress,
        threads: get_parameter(&matches, "threads", 1)?,
    });

    let (send_queue, stats_rx, wait_group) = start_workers(&parameters);

    let mut include_filters: Vec<String> = match matches.values_of("filters") {
        Some(values) => Ok(values.map(|s| s.to_owned()).collect()),
        None => Err(ErrorKind::InvalidUsage),
    }?;

    let mut builder = GlobSetBuilder::new();
    fix_filters(&mut include_filters);
    for filter in include_filters.iter() {
        let glob = GlobBuilder::new(filter)
            .case_insensitive(!case_sensitive)
            .literal_separator(true)
            .build().map_err(|_| ErrorKind::InvalidIncludeFilter)?;
        builder.add(glob);
    }
    let globset = builder.build().map_err(|_| ErrorKind::InvalidIncludeFilter)?;

    // Convert filters to paths and deal out conversion jobs
    dispatch_jobs(send_queue, include_filters, globset/*, exclude_filters*/)?;

    // Wait for all jobs to finish
    wait_group.wait();

    // Merge statistics from all threads
    if show_summary {
        let mut stats = Statistics::new();
        while let Ok(thread_stats) = stats_rx.recv() {
            stats.merge(&thread_stats);
        }

        println!("{}", stats);
    }

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
        // We are ignoring .files and .directories
        // We may add a command-line switch to control this behavior in the future
        return Ok(());
    }

    if path.is_dir() {
        for child in path.read_dir()? {
            let child_path = child?.path();
            yield_file(child_path, globset, callback)?;
        }
    }
    else {
        // I'm presuming the binary search in is_blacklisted is faster
        // than globset.is_match, but we should benchmark it at some point
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
            None => break, // No more tasks
        };

        // In a nested function so we can handle errors centrally
        fn compress_single(src: &ThreadParam, params: &Parameters, mut local_stats: &mut Statistics) -> Result<()> {
            let dst_path = format!("{}.{}",
                                   src.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?,
                                   params.extension);
            let dst = Path::new(&dst_path);

            // Again, in a scope for error handling
            |local_stats: &mut Statistics| -> Result<()> {
                    let src_metadata = std::fs::metadata(src)?;

                    // Don't compress files that are already compressed that haven't changed
                    if let Ok(dst_metadata) = std::fs::metadata(dst) {
                        // The destination already exists
                        let src_seconds = src_metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
                        let dst_seconds = dst_metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();
                        match src_seconds == dst_seconds {
                            true => {
                                local_stats.update(src_metadata.len(), dst_metadata.len(), false);
                                // No need to recompress
                                return Ok(());
                            },
                            false => {
                                // Return an error if we can't remove the file
                                std::fs::remove_file(dst)?;
                            }
                        };
                    }

                    if params.show_progress {
                        println!("{}", src.display());
                    }
                    params.compressor.compress(src.as_path(), dst, params.quality)?;
                    let dst_metadata = std::fs::metadata(dst)?;
                    local_stats.update(src_metadata.len(), dst_metadata.len(), true);
                    let src_modified = filetime::FileTime::from_last_modification_time(&src_metadata);
                    filetime::set_file_times(dst, filetime::FileTime::zero(), src_modified).unwrap_or_default();

                    Ok(())
                }(&mut local_stats)
                .map_err(|e| {
                    // Try deleting the invalid destination file, but don't care if we can't
                    std::fs::remove_file(dst).unwrap_or_default();
                    e // Bubble up the same error
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

// Prepends ./ to relative paths
fn fix_filters(filters: &mut Vec<String>) {
    for i in 0..filters.len() {
        let new_path;
        {
            let ref path = filters[i];
            match path.chars().next().expect("Received blank filter!") {
                '.' | '/' => continue,
                _ => new_path = format!("./{}", path) // Use un-prefixed path
            }
        }
        filters[i] = new_path;
    }
}

// Given a list of filters, extracts the directories that should be searched.
// TODO: Also provide info about to what depth they should be recursed.
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
            // Take everything until the first expression
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
                        let mut pb = PathBuf::from("./");
                        pb.push(dir);
                        pb
                    }
                },
                None => {
                    insert_path(filter, PathBuf::from("./"));
                    continue;
                }
            };

            if dir.to_str().ok_or(ErrorKind::InvalidCharactersInPath)?.ends_with(filter) {
                // The "dir" is actually a full path to a single file, return it as-is.
                insert_path(filter, dir);
                continue;
            }

            if last_char == Some('/') {
                // Dir is a already a directory, return it as-is.
                insert_path(filter, dir);
                continue;
            }

            // We need to extract the directory from the path we have
            let dir = match PathBuf::from(dir).parent() {
                Some(parent) => parent.to_path_buf(),
                None => PathBuf::from("./"),
            };

            insert_path(filter, dir);
        }
    }

    debug(&format!("final search paths: {:?}", dirs));

    Ok(dirs)
}
